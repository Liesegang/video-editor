use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::state::context_types::{
    ContainerResizeEdge, ContainerResizeState, ContextMenuState, NodeEditorEditableWire,
    NodeEditorNodeDragOrigin, NodeEditorNormalConnectGesture, NodeEditorPendingEdit,
    NodeEditorReparentGesture, NodeEditorState, NodeEditorWireContextMenu, NodeEditorWireDragKind,
    NodeEditorWireGesture, NodeEditorWireKnifeGesture, SelectionTarget,
};
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};
use crate::ui::widgets::searchable_context_menu::{
    searchable_popup_placement, show_searchable_items_with_qa, SearchableItem,
};
use eframe::egui::{self, Color32};
use egui_snarl::{
    ui::{
        BackgroundPattern, NodeLayout, PinInfo, PinWireInfo, SnarlPin, SnarlStyle, SnarlViewer,
        WireStyle,
    },
    InPin, OutPin, Snarl,
};
use library::model::project::{
    ContainerImageSourceKind, PortAddress, PortDataType, PortDirection, PortMultiplicity,
    PortOwner, PortSide, MERGE_IMAGES_PORT,
};
use library::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use library::model::{
    BlendMode, Clip, GeneratorContent, Node, NodeContainer, NodeContent, NodeGraphBundle, Project,
};
use library::plugin::{
    property_name_from_port, PluginManager, DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY,
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY,
    STYLE_APPLY_OPERATION, STYLE_CATEGORY,
};
use library::EditorService;
use ordered_float::OrderedFloat;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

mod canvas;
mod property_evaluation;

use canvas::{
    adaptive_grid_spacing, node_editor_canvas_metadata, node_editor_details_visible,
    node_editor_port_interactions_enabled, node_editor_resize_interactions_enabled,
    node_editor_snarl_style, sanitize_node_editor_transform, sanitized_node_editor_scale,
    screen_stroke_in_graph_units,
};
#[cfg(test)]
use canvas::{
    GRID_TARGET_SCREEN_SPACING, NODE_EDITOR_DETAIL_SCALE, NODE_EDITOR_MAX_SCALE,
    NODE_EDITOR_MAX_TRANSLATION, NODE_EDITOR_MIN_SCALE, NODE_EDITOR_RESIZE_INTERACTION_SCALE,
};
use property_evaluation::{evaluate_node_property, render_node_property_issue};
const CONTAINER_HEADER_HEIGHT: f32 = 64.0;
const CONTAINER_CONTROL_OFFSET: egui::Vec2 = egui::vec2(14.0, 10.0);
const CONTAINER_PORT_Y: f32 = 86.0;
const EMBEDDED_PORT_LABEL_INSET: f32 = 18.0;
const RESIZE_HIT_WIDTH: f32 = 7.0;
const RESIZE_CORNER_SIZE: f32 = 15.0;
const NODE_BODY_WIDTH: f32 = 200.0;
const MERGE_BODY_WIDTH: f32 = 230.0;
const NODE_HEADER_WIDTH: f32 = 190.0;
const PORT_LABEL_WIDTH: f32 = 96.0;
const PORT_ROW_HEIGHT: f32 = 22.0;
const PROPERTY_LABEL_WIDTH: f32 = 58.0;
const INLINE_CONTROL_WIDTH: f32 = 126.0;
const WIRE_HIT_RADIUS: f32 = 8.0;
const WIRE_ENDPOINT_RADIUS: f32 = 12.0;
/// The custom endpoint-reconnect path accepts a drop this many screen points
/// around a rendered port, including at overview zoom where Snarl's normal
/// port interaction is deliberately disabled.
const WIRE_PORT_DROP_RADIUS: f32 = 5.0;
const WIRE_DRAG_THRESHOLD: f32 = 6.0;
/// Crossing a container boundary is a semantic edit, so a tiny title jitter
/// must not change ownership. The threshold is measured in screen points and
/// therefore remains stable under Node Editor zoom.
const NODE_REPARENT_DRAG_THRESHOLD: f32 = 8.0;
/// A pointer inside a candidate can select it before the Node center crosses
/// only when a meaningful portion of the final rendered Node is already in
/// that candidate. This keeps header-offset drags usable without pointer-only
/// reparenting at a boundary.
const NODE_REPARENT_POINTER_OVERLAP_THRESHOLD: f32 = 0.35;
const MIN_CONTAINER_SIZE: egui::Vec2 = egui::vec2(360.0, 220.0);
const AUTO_LAYOUT_COLUMN_GAP: f32 = 112.0;
const AUTO_LAYOUT_ROW_GAP: f32 = 52.0;
const AUTO_LAYOUT_NODE_PADDING: f32 = 24.0;
const DETACHED_GRAPH_NODE_GAP: f32 = AUTO_LAYOUT_NODE_PADDING + 0.5;
const AUTO_LAYOUT_CLIP_GAP: f32 = 64.0;
const AUTO_LAYOUT_TRACK_GAP: f32 = 80.0;
const CONTAINER_CONTROL_CHROME_HEIGHT: f32 = 54.0;
const CONTAINER_STANDARD_BODY_ROWS: f32 = 2.0;
const CONTAINER_CLIP_BODY_ROWS: f32 = 6.0;
const fn reserved_control_top(body_rows: f32) -> f32 {
    CONTAINER_CONTROL_OFFSET.y
        + CONTAINER_CONTROL_CHROME_HEIGHT
        + body_rows * PORT_ROW_HEIGHT
        + AUTO_LAYOUT_NODE_PADDING
}
const AUTO_LAYOUT_COMPOSITION_TOP: f32 = reserved_control_top(CONTAINER_STANDARD_BODY_ROWS);
const AUTO_LAYOUT_COMPOSITION_LEFT: f32 = 190.0;
const AUTO_LAYOUT_COMPOSITION_RIGHT: f32 = 100.0;
const AUTO_LAYOUT_COMPOSITION_BOTTOM: f32 = 100.0;
const AUTO_LAYOUT_TRACK_TOP: f32 = reserved_control_top(CONTAINER_STANDARD_BODY_ROWS);
const AUTO_LAYOUT_CLIP_TOP: f32 = reserved_control_top(CONTAINER_CLIP_BODY_ROWS);
const AUTO_LAYOUT_TRACK_LEFT: f32 = 184.0;
const AUTO_LAYOUT_TRACK_RIGHT: f32 = 150.0;
const AUTO_LAYOUT_TRACK_BOTTOM: f32 = 70.0;

/// Ephemeral Snarl payload. It contains identity and visual role only; all
/// editable values continue to live in `Project`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GraphItem {
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
enum PortAnchorKind {
    ExternalInputs,
    InternalMetadata,
    ImageSink,
    ExternalImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    Composition,
    Track,
    Clip,
}

#[derive(Clone, Debug)]
struct ContainerVisual {
    owner: PortOwner,
    kind: ContainerKind,
    position: [f32; 2],
    size: [f32; 2],
    collapsed: bool,
}

impl ContainerVisual {
    fn rect(&self) -> egui::Rect {
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
struct PinDefinition {
    key: String,
    name: String,
    data_type: PortDataType,
}

struct QaPin {
    info: PinInfo,
    component_id: String,
    to_global: egui::emath::TSTransform,
    graph_center: Option<egui::Pos2>,
    address: Option<PortAddress>,
    direction: PortDirection,
    connected: bool,
    canvas_clip: egui::Rect,
    rendered_ports: Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderedPortKey {
    address: PortAddress,
    direction: PortDirection,
}

#[derive(Clone, Debug)]
struct RenderedEdge {
    kind: RenderedEdgeKind,
    start: egui::Pos2,
    control_a: egui::Pos2,
    control_b: egui::Pos2,
    end: egui::Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RenderedEdgeKind {
    ProjectConnection { connection_id: Uuid },
    OutputBinding { owner: PortOwner, node_id: Uuid },
    DerivedOutput { owner: PortOwner, source: PortOwner },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WireSecondaryClickHit {
    Editable(NodeEditorEditableWire),
    DisplayOnly,
}

impl RenderedEdgeKind {
    fn metadata_kind(self) -> &'static str {
        match self {
            Self::ProjectConnection { .. } => "explicit",
            Self::OutputBinding { .. } => "output_binding",
            Self::DerivedOutput { .. } => "derived_output",
        }
    }

    fn editable_wire(self) -> Option<NodeEditorEditableWire> {
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

    fn connection_id(self) -> Option<Uuid> {
        match self {
            Self::ProjectConnection { connection_id } => Some(connection_id),
            Self::OutputBinding { .. } | Self::DerivedOutput { .. } => None,
        }
    }

    fn blocked_reason(self) -> Option<&'static str> {
        matches!(self, Self::DerivedOutput { .. }).then_some(
            "Derived wire follows authoritative containment; reparent or remove the child instead",
        )
    }
}

struct EdgeComponent<'a> {
    id: String,
    kind: RenderedEdgeKind,
    from: &'a PortAddress,
    to: &'a PortAddress,
    wire_color: Color32,
    authored_order: Option<i64>,
    back_to_front_index: Option<usize>,
    layer_count: Option<usize>,
    authored_blend_mode: Option<&'static str>,
    authored_blend_available: bool,
}

#[derive(Clone, Copy)]
struct OverviewWirePainter<'a> {
    painter: &'a egui::Painter,
    to_global: egui::emath::TSTransform,
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

#[cfg(test)]
thread_local! {
    static TEST_RENDER_RECTS: std::cell::RefCell<HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(HashMap::new());
    static TEST_RENDER_METADATA: std::cell::RefCell<HashMap<String, serde_json::Value>> =
        std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
fn capture_test_rect(id: &str, rect: egui::Rect) {
    TEST_RENDER_RECTS.with(|rects| {
        rects.borrow_mut().insert(id.to_string(), rect);
    });
}

#[cfg(test)]
fn capture_test_metadata(id: &str, metadata: &serde_json::Value) {
    TEST_RENDER_METADATA.with(|entries| {
        entries
            .borrow_mut()
            .insert(id.to_string(), metadata.clone());
    });
}

#[cfg(test)]
fn reset_test_rects() {
    TEST_RENDER_RECTS.with(|rects| rects.borrow_mut().clear());
    TEST_RENDER_METADATA.with(|entries| entries.borrow_mut().clear());
}

#[cfg(test)]
fn test_rect(id: &str) -> Option<egui::Rect> {
    TEST_RENDER_RECTS.with(|rects| rects.borrow().get(id).copied())
}

#[cfg(test)]
fn test_rects() -> HashMap<String, egui::Rect> {
    TEST_RENDER_RECTS.with(|rects| rects.borrow().clone())
}

#[cfg(test)]
fn test_metadata(id: &str) -> Option<serde_json::Value> {
    TEST_RENDER_METADATA.with(|entries| entries.borrow().get(id).cloned())
}

#[derive(Debug)]
enum NodeEdit {
    Connect {
        from: PortAddress,
        to: PortAddress,
    },
    Disconnect {
        from: PortAddress,
        to: PortAddress,
    },
    DisconnectConnection {
        connection_id: Uuid,
    },
    DisconnectWires {
        wires: Vec<NodeEditorEditableWire>,
    },
    ReconnectConnection {
        connection_id: Uuid,
        from: PortAddress,
        to: PortAddress,
    },
    SetConnectionBlendMode {
        connection_id: Uuid,
        blend_mode: BlendMode,
    },
    ReorderConnection {
        connection_id: Uuid,
        new_order: i64,
    },
    SpliceExistingNode {
        connection_id: Uuid,
        node_id: Uuid,
    },
    InsertNodeOnConnection {
        connection_id: Uuid,
        node: Box<Node>,
        position: egui::Pos2,
        composition_id: Uuid,
    },
    SetOutputNode {
        owner: PortOwner,
        node_id: Option<Uuid>,
    },
    Delete {
        owner: PortOwner,
    },
    SetEnabled {
        node_id: Uuid,
        enabled: bool,
    },
    RenameContainer {
        owner: PortOwner,
        name: String,
    },
    ResizeContainer {
        owner: PortOwner,
        size: [f32; 2],
    },
    ToggleContainer {
        owner: PortOwner,
    },
    Rename {
        node_id: Uuid,
        name: String,
    },
    SetProperty {
        owner: PortOwner,
        key: String,
        time: f64,
        value: PropertyValue,
    },
}

#[derive(Debug)]
enum QueuedNodeEdit {
    Atomic(NodeEdit),
    Continuous {
        pending: NodeEditorPendingEdit,
        edit: Option<NodeEdit>,
        finished: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum LayoutEdit {
    MoveNode {
        node_id: Uuid,
        position: [f32; 2],
    },
    MoveContainer {
        owner: PortOwner,
        delta: [f32; 2],
    },
    ResizeContainer {
        owner: PortOwner,
        position: [f32; 2],
        size: [f32; 2],
    },
}

#[derive(Clone, Debug)]
enum AutoLayoutScope {
    All,
    Selection(Vec<Uuid>),
    Container(PortOwner),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ContainerLayout {
    position: [f32; 2],
    size: [f32; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AutoLayoutPlan {
    node_positions: BTreeMap<Uuid, [f32; 2]>,
    clip_layouts: BTreeMap<Uuid, ContainerLayout>,
    track_layouts: BTreeMap<Uuid, ContainerLayout>,
    composition_size: Option<[f32; 2]>,
}

/// The Snarl payload contains only stable IDs. All labels, pins, positions and
/// connections are read from `Project` for this frame, so Snarl never becomes
/// another editable project model.
struct ProjectNodeViewer<'a> {
    project: &'a Project,
    plugin_manager: Option<&'a PluginManager>,
    containers: &'a [ContainerVisual],
    edits: &'a mut Vec<QueuedNodeEdit>,
    pending_navigation: &'a mut Option<Uuid>,
    pending_selection: &'a mut Option<PortOwner>,
    current_time: f64,
    context_menu_exclusion_rects: &'a mut Vec<egui::Rect>,
    wire_context_request: &'a mut Option<NodeEditorEditableWire>,
    suppress_wire_connect: bool,
    locked_canvas_transform: Option<egui::emath::TSTransform>,
    to_global: &'a mut egui::emath::TSTransform,
    canvas_clip: &'a mut egui::Rect,
    rendered_ports: Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    /// Exact graph-space rectangles reported by Snarl after layout. These are
    /// the geometry authority for drop targeting; estimated Node sizes are not
    /// precise enough around nested container boundaries.
    rendered_node_rects: Arc<Mutex<HashMap<Uuid, egui::Rect>>>,
}

impl SnarlViewer<GraphItem> for ProjectNodeViewer<'_> {
    fn node_layout(
        &mut self,
        _default: NodeLayout,
        _node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<GraphItem>,
    ) -> NodeLayout {
        // Coil keeps inputs on the left and outputs on the right. Each side is
        // a top-down list, so pins remain one-per-row without turning the data
        // flow into a top-to-bottom graph. Width is bounded by the label/body
        // helpers below instead of changing pin sides.
        NodeLayout::coil()
            .with_min_pin_row_height(PORT_ROW_HEIGHT)
            .with_equal_pin_rows()
    }

    fn node_frame(
        &mut self,
        default: egui::Frame,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<GraphItem>,
    ) -> egui::Frame {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return default;
        };
        match item {
            GraphItem::Container(_) | GraphItem::PortAnchor { .. } => egui::Frame::NONE,
            GraphItem::Node(project_node_id) => {
                let palette = node_palette(self.project, project_node_id);
                let inactive = graph_item_inactive(self.project, item, self.current_time);
                let fill = if inactive {
                    palette.body.gamma_multiply(0.42)
                } else {
                    palette.body
                };
                let stroke = if inactive {
                    palette.accent.gamma_multiply(0.48)
                } else {
                    palette.accent
                };
                let stroke_width = if node_editor_details_visible(self.to_global.scaling) {
                    1.25
                } else {
                    screen_stroke_in_graph_units(1.1, self.to_global.scaling)
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(9, 8))
                    .corner_radius(10)
                    .fill(fill)
                    .stroke(egui::Stroke::new(stroke_width, stroke))
            }
        }
    }

    fn header_frame(
        &mut self,
        default: egui::Frame,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<GraphItem>,
    ) -> egui::Frame {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return default;
        };
        match item {
            GraphItem::Container(_) | GraphItem::PortAnchor { .. } => egui::Frame::NONE,
            GraphItem::Node(project_node_id) => {
                let palette = node_palette(self.project, project_node_id);
                let fill = if graph_item_inactive(self.project, item, self.current_time) {
                    palette.header.gamma_multiply(0.42)
                } else {
                    palette.header
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(9, 7))
                    .corner_radius(egui::CornerRadius {
                        nw: 9,
                        ne: 9,
                        sw: 3,
                        se: 3,
                    })
                    .fill(fill)
            }
        }
    }

    fn show_header(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };

        match item {
            GraphItem::Node(project_node_id) => {
                let palette = node_palette(self.project, project_node_id);
                ui.set_min_width(NODE_HEADER_WIDTH);
                let response = if node_editor_details_visible(self.to_global.scaling) {
                    ui.horizontal(|ui| {
                        non_selectable_label(
                            ui,
                            egui::RichText::new(node_icon(self.project, project_node_id))
                                .color(palette.accent)
                                .strong(),
                        );
                        bounded_strong_non_selectable_label(
                            ui,
                            node_title(self.project, project_node_id),
                            NODE_HEADER_WIDTH - 48.0,
                        );
                        let status = if graph_item_inactive(
                            self.project,
                            GraphItem::Node(project_node_id),
                            self.current_time,
                        ) {
                            "○"
                        } else {
                            "●"
                        };
                        non_selectable_label(ui, egui::RichText::new(status).color(palette.accent));
                    })
                    .response
                } else {
                    ui.allocate_response(
                        egui::vec2(NODE_HEADER_WIDTH, PORT_ROW_HEIGHT),
                        egui::Sense::hover(),
                    )
                };
                let response = graph_item_inactive_reason(
                    self.project,
                    GraphItem::Node(project_node_id),
                    self.current_time,
                )
                .map_or(response.clone(), |reason| {
                    response.on_hover_text(reason.tooltip())
                });
                let unclipped_header_rect = *self.to_global * response.rect;
                let header_rect = clipped_qa_rect(unclipped_header_rect, *self.canvas_clip);
                let coordinate_clicked = ui.input(|input| {
                    input.pointer.primary_clicked()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| header_rect.contains(position))
                });
                let coordinate_double_clicked = ui.input(|input| {
                    input
                        .pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| header_rect.contains(position))
                });
                let component_id = format!("node_editor.node_header:{project_node_id}");
                #[cfg(test)]
                capture_test_rect(&component_id, header_rect);
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node_header",
                    header_rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "node_id": project_node_id,
                        "hovered": response.hovered(),
                        "unclipped_rect": qa_rect_metadata(unclipped_header_rect),
                        "visible_in_canvas": header_rect.is_positive(),
                    })),
                );
                if coordinate_clicked {
                    *self.pending_selection = Some(PortOwner::Node(project_node_id));
                }
                if coordinate_double_clicked {
                    if let Some(node) = self.project.get_node(project_node_id) {
                        if let NodeContent::Reference(reference) = node.content() {
                            *self.pending_navigation = Some(reference.target_id);
                        }
                    }
                }
            }
            GraphItem::Container(owner) => {
                let collapsed = container_collapsed(self.project, owner).unwrap_or(false);
                let header_width = container_name_and_size(self.project, owner)
                    .map_or(240.0, |(_, size)| (size[0] - 28.0).max(240.0));
                ui.set_min_width(header_width);
                let response = if node_editor_details_visible(self.to_global.scaling) {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(if collapsed { "▸" } else { "▾" })
                            .on_hover_text(if collapsed { "Expand" } else { "Collapse" })
                            .clicked()
                        {
                            self.edits
                                .push(QueuedNodeEdit::Atomic(NodeEdit::ToggleContainer { owner }));
                        }
                        non_selectable_label(
                            ui,
                            egui::RichText::new(container_icon(owner)).strong(),
                        );
                        strong_non_selectable_label(ui, container_title(self.project, owner));
                    })
                    .response
                } else {
                    ui.allocate_response(
                        egui::vec2(header_width, PORT_ROW_HEIGHT),
                        egui::Sense::hover(),
                    )
                };
                let response = if container_inactive(self.project, owner, self.current_time) {
                    response
                        .on_hover_text("No output (outside Clip range). The Clip remains editable.")
                } else {
                    response
                };
                let unclipped_header_rect = *self.to_global * response.rect;
                let header_rect = clipped_qa_rect(unclipped_header_rect, *self.canvas_clip);
                let component_id =
                    format!("node_editor.container_header.{}", qa_container_key(owner));
                #[cfg(test)]
                capture_test_rect(&component_id, header_rect);
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node_container_header",
                    header_rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "owner": qa_container_key(owner),
                        "unclipped_rect": qa_rect_metadata(unclipped_header_rect),
                        "visible_in_canvas": header_rect.is_positive(),
                    })),
                );
                if ui.input(|input| {
                    input.pointer.primary_clicked()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| header_rect.contains(position))
                }) {
                    *self.pending_selection = Some(owner);
                }
            }
            GraphItem::PortAnchor { .. } => {
                ui.allocate_space(egui::Vec2::ZERO);
            }
        }
    }

    fn title(&mut self, item: &GraphItem) -> String {
        graph_item_title(self.project, *item)
    }

    fn inputs(&mut self, item: &GraphItem) -> usize {
        input_definitions(self.project, *item).len()
    }

    fn outputs(&mut self, item: &GraphItem) -> usize {
        output_definitions(self.project, *item).len()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) -> impl SnarlPin + 'static {
        let item = snarl.get_node(pin.id.node).copied();
        let definition = snarl
            .get_node(pin.id.node)
            .and_then(|item| {
                input_definitions(self.project, *item)
                    .get(pin.id.input)
                    .cloned()
            })
            .unwrap_or_else(|| PinDefinition {
                key: "missing".to_string(),
                name: "Input".to_string(),
                data_type: PortDataType::Any,
            });
        let connected = !pin.remotes.is_empty();
        if let Some(GraphItem::Node(node_id)) = item {
            if node_editor_details_visible(self.to_global.scaling) {
                let property_key = property_name_from_port(&definition.key)
                    .unwrap_or(&definition.key)
                    .to_string();
                let property_definition = self.plugin_manager.and_then(|manager| {
                    plugin_operation_property_definition(
                        manager,
                        self.project.get_node(node_id)?,
                        &property_key,
                    )
                });
                self.show_node_input_row(
                    ui,
                    node_id,
                    &definition,
                    &property_key,
                    property_definition.as_ref(),
                    connected,
                );
            } else {
                ui.allocate_space(egui::vec2(PORT_LABEL_WIDTH + 80.0, PORT_ROW_HEIGHT));
            }
        } else {
            ui.allocate_space(egui::vec2(0.0, PORT_ROW_HEIGHT));
        }
        let address = item
            .and_then(graph_item_owner)
            .map(|owner| PortAddress::new(owner, definition.key.clone()));
        QaPin {
            info: pin_info(definition.data_type, connected),
            component_id: qa_port_id(self.project, item, "input", &definition.key),
            to_global: *self.to_global,
            graph_center: embedded_pin_center(
                self.containers,
                item,
                PortDirection::Input,
                pin.id.input,
            ),
            address,
            direction: PortDirection::Input,
            connected,
            canvas_clip: *self.canvas_clip,
            rendered_ports: Arc::clone(&self.rendered_ports),
        }
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) -> impl SnarlPin + 'static {
        let item = snarl.get_node(pin.id.node).copied();
        let definition = snarl
            .get_node(pin.id.node)
            .and_then(|item| {
                output_definitions(self.project, *item)
                    .get(pin.id.output)
                    .cloned()
            })
            .unwrap_or_else(|| PinDefinition {
                key: "missing".to_string(),
                name: "Output".to_string(),
                data_type: PortDataType::Any,
            });
        if matches!(item, Some(GraphItem::Node(_))) {
            if node_editor_details_visible(self.to_global.scaling) {
                bounded_non_selectable_label(
                    ui,
                    definition.name.clone(),
                    port_label_width(item),
                    egui::Align::RIGHT,
                );
            } else {
                ui.allocate_space(egui::vec2(PORT_LABEL_WIDTH, PORT_ROW_HEIGHT));
            }
        } else {
            ui.allocate_space(egui::vec2(0.0, PORT_ROW_HEIGHT));
        }
        let address = item
            .and_then(graph_item_owner)
            .map(|owner| PortAddress::new(owner, definition.key.clone()));
        let connected = !pin.remotes.is_empty();
        QaPin {
            info: pin_info(definition.data_type, connected),
            component_id: qa_port_id(self.project, item, "output", &definition.key),
            to_global: *self.to_global,
            graph_center: embedded_pin_center(
                self.containers,
                item,
                PortDirection::Output,
                pin.id.output,
            ),
            address,
            direction: PortDirection::Output,
            connected,
            canvas_clip: *self.canvas_clip,
            rendered_ports: Arc::clone(&self.rendered_ports),
        }
    }

    fn has_body(&mut self, item: &GraphItem) -> bool {
        matches!(
            item,
            GraphItem::Node(node_id)
                if self
                    .project
                    .get_node(*node_id)
                    .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
        )
    }

    fn show_body(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };
        if let GraphItem::Container(owner) = item {
            ui.vertical(|ui| {
                ui.set_width(258.0);
                self.show_container_body(owner, ui);
            });
            return;
        }
        let GraphItem::Node(project_node_id) = item else {
            return;
        };
        if self
            .project
            .get_node(project_node_id)
            .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
        {
            ui.vertical(|ui| {
                ui.set_width(MERGE_BODY_WIDTH);
                self.show_merge_layers(project_node_id, ui);
            });
            return;
        }
        ui.vertical(|ui| {
            ui.set_width(NODE_BODY_WIDTH);
            let Some(node) = self.project.get_node(project_node_id) else {
                return;
            };

            let mut name = node.name.clone();
            ui.horizontal(|ui| {
                property_label(ui, "Name");
                let response = ui.add_sized(
                    [INLINE_CONTROL_WIDTH, PORT_ROW_HEIGHT],
                    egui::TextEdit::singleline(&mut name),
                );
                let finished = continuous_response_finished(ui, &response);
                let edit = response.changed().then_some(NodeEdit::Rename {
                    node_id: project_node_id,
                    name,
                });
                self.queue_continuous_edit(
                    PortOwner::Node(project_node_id),
                    "$name",
                    edit,
                    finished,
                );
            });

            match node.content() {
                NodeContent::Generator(GeneratorContent::Text) => {
                    self.edit_string_property(ui, project_node_id, node, "text", "Text", "");
                    self.edit_string_property(
                        ui,
                        project_node_id,
                        node,
                        "font_family",
                        "Font",
                        library::editor::project_service::DEFAULT_TEXT_FONT,
                    );
                }
                NodeContent::Generator(GeneratorContent::Shape) => {
                    self.edit_string_property(
                        ui,
                        project_node_id,
                        node,
                        "path",
                        "Path",
                        library::editor::project_service::DEFAULT_SHAPE_PATH,
                    );
                }
                NodeContent::Generator(GeneratorContent::SkSL) => {
                    self.edit_string_property(
                        ui,
                        project_node_id,
                        node,
                        "shader",
                        "Shader",
                        library::editor::project_service::DEFAULT_SKSL_SHADER,
                    );
                }
                NodeContent::Generator(GeneratorContent::Solid) => {
                    let property_time =
                        node_property_time(self.project, project_node_id, self.current_time);
                    let evaluated = node.properties().get("color").map(|property| {
                        evaluate_node_property(
                            self.project,
                            self.plugin_manager,
                            project_node_id,
                            property,
                            property_time,
                        )
                    });
                    let color = evaluated
                        .as_ref()
                        .and_then(|evaluated| evaluated.value())
                        .and_then(|value| value.get_as::<library::model::frame::color::Color>())
                        .unwrap_or(library::model::frame::color::Color {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 255,
                        });
                    let mut edited =
                        Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
                    ui.horizontal(|ui| {
                        property_label(ui, "Color");
                        if let Some(issue) =
                            evaluated.as_ref().and_then(|evaluated| evaluated.issue())
                        {
                            render_node_property_issue(ui, project_node_id, "color", issue);
                        }
                        let (response, popup_closed) =
                            continuous_color_edit_button(ui, &mut edited);
                        let finished = popup_closed || continuous_response_finished(ui, &response);
                        let edit = response.changed().then(|| NodeEdit::SetProperty {
                            owner: PortOwner::Node(project_node_id),
                            key: "color".into(),
                            time: property_time,
                            value: PropertyValue::Color(library::model::frame::color::Color {
                                r: edited.r(),
                                g: edited.g(),
                                b: edited.b(),
                                a: edited.a(),
                            }),
                        });
                        self.queue_continuous_edit(
                            PortOwner::Node(project_node_id),
                            "color",
                            edit,
                            finished,
                        );
                    });
                }
                NodeContent::PluginOperation(operation) => {
                    ui.horizontal(|ui| {
                        property_label(ui, "Category");
                        bounded_non_selectable_label(
                            ui,
                            &operation.category,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                    ui.horizontal(|ui| {
                        property_label(ui, "Component");
                        bounded_non_selectable_label(
                            ui,
                            &operation.component_id,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                    ui.horizontal(|ui| {
                        property_label(ui, "Operation");
                        bounded_non_selectable_label(
                            ui,
                            &operation.operation,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                }
                NodeContent::Value(value) => {
                    ui.horizontal(|ui| {
                        property_label(ui, "Category");
                        bounded_non_selectable_label(
                            ui,
                            VALUE_NODE_CATEGORY_LABEL,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                    ui.horizontal(|ui| {
                        property_label(ui, "Operation");
                        bounded_non_selectable_label(
                            ui,
                            value_operation_label(*value),
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                }
                NodeContent::Media(_) | NodeContent::Reference(_) | NodeContent::Merge => {}
            }
        });
    }

    fn has_node_menu(&mut self, item: &GraphItem) -> bool {
        matches!(
            item,
            GraphItem::Node(_)
                | GraphItem::Container(PortOwner::Track(_))
                | GraphItem::Container(PortOwner::Clip(_))
        )
    }

    fn show_node_menu(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };
        let delete_target = match item {
            GraphItem::Node(node_id) => Some((PortOwner::Node(node_id), "Delete Node")),
            GraphItem::Container(PortOwner::Track(track_id)) => {
                Some((PortOwner::Track(track_id), "Delete Track"))
            }
            GraphItem::Container(PortOwner::Clip(clip_id)) => {
                Some((PortOwner::Clip(clip_id), "Delete Clip"))
            }
            _ => None,
        };
        if let GraphItem::Node(project_node_id) = item {
            if let Some(node) = self.project.get_node(project_node_id) {
                let enabled = !node.enabled;
                let label = if enabled {
                    "Enable Node"
                } else {
                    "Disable Node"
                };
                let response = ui.button(label);
                crate::qa::register_component_with_metadata(
                    format!("node_editor.menu.toggle_enabled.node:{project_node_id}"),
                    "node_editor_menu_item",
                    response.rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "action": if enabled { "enable" } else { "disable" },
                        "owner": qa_container_key(PortOwner::Node(project_node_id)),
                        "enabled": enabled,
                    })),
                );
                if response.clicked() {
                    self.edits
                        .push(QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
                            node_id: project_node_id,
                            enabled,
                        }));
                    ui.close();
                    return;
                }
            }
        }
        if let Some((owner, label)) = delete_target {
            let response = ui.button(label);
            crate::qa::register_component_with_metadata(
                format!("node_editor.menu.delete.{}", qa_container_key(owner)),
                "node_editor_menu_item",
                response.rect,
                response.enabled(),
                Some(serde_json::json!({
                    "action": "delete",
                    "owner": qa_container_key(owner),
                })),
            );
            if response.clicked() {
                self.edits
                    .push(QueuedNodeEdit::Atomic(NodeEdit::Delete { owner }));
                ui.close();
            }
        }
    }

    fn final_node_rect(
        &mut self,
        node_id: egui_snarl::NodeId,
        rect: egui::Rect,
        _ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };
        let graph_rect = rect;
        let unclipped_rect = *self.to_global * graph_rect;
        let rect = clipped_qa_rect(unclipped_rect, *self.canvas_clip);
        // Snarl owns secondary-click interaction on every rendered graph item,
        // including the otherwise transparent container and port-anchor
        // nodes. Keep these hit rectangles in graph space: the global Create
        // menu applies the inverse canvas transform before consulting them.
        // Container backdrops are painted separately and deliberately are not
        // included, so right-clicking an empty container body still creates a
        // Node in that container.
        self.context_menu_exclusion_rects.push(graph_rect);
        match item {
            GraphItem::Node(id) => {
                if let Ok(mut node_rects) = self.rendered_node_rects.lock() {
                    node_rects.insert(id, graph_rect);
                }
                #[cfg(test)]
                capture_test_rect(&format!("node_editor.node:{id}"), rect);
                crate::qa::register_component_with_metadata(
                    format!("node_editor.node:{id}"),
                    "node",
                    rect,
                    true,
                    Some(serde_json::json!({
                        "node_id": id,
                        "inactive": graph_item_inactive(
                            self.project,
                            GraphItem::Node(id),
                            self.current_time,
                        ),
                        "inactive_reason": graph_item_inactive_reason(
                            self.project,
                            GraphItem::Node(id),
                            self.current_time,
                        ).map(GraphItemInactiveReason::as_str),
                        "unclipped_rect": qa_rect_metadata(unclipped_rect),
                        "visible_in_canvas": rect.is_positive(),
                    })),
                )
            }
            GraphItem::Container(_) | GraphItem::PortAnchor { .. } => {}
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GraphItem>) {
        if self.suppress_wire_connect {
            return;
        }
        if let Some(edit) = edit_for_wire(
            self.project,
            snarl,
            from.id.node,
            from.id.output,
            to.id.node,
            to.id.input,
            true,
        ) {
            self.edits.push(QueuedNodeEdit::Atomic(edit));
            snarl.connect(from.id, to.id);
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GraphItem>) {
        let edit = edit_for_wire(
            self.project,
            snarl,
            from.id.node,
            from.id.output,
            to.id.node,
            to.id.input,
            false,
        );
        let context_target = match &edit {
            Some(NodeEdit::Disconnect { from, to }) => self
                .project
                .connections
                .iter()
                .find(|connection| connection.from == *from && connection.to == *to)
                .map(|connection| NodeEditorEditableWire::ProjectConnection {
                    connection_id: connection.id,
                }),
            Some(NodeEdit::SetOutputNode {
                owner,
                node_id: None,
            }) => container_output_node_id(self.project, *owner).map(|node_id| {
                NodeEditorEditableWire::OutputBinding {
                    owner: *owner,
                    node_id,
                }
            }),
            _ => None,
        };
        if let Some(target) = context_target {
            *self.wire_context_request = Some(target);
            return;
        }
        if let Some(edit) = edit {
            self.edits.push(QueuedNodeEdit::Atomic(edit));
        }
        snarl.disconnect(from.id, to.id);
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<GraphItem>) {
        for remote in &pin.remotes {
            if let Some(edit) = edit_for_wire(
                self.project,
                snarl,
                pin.id.node,
                pin.id.output,
                remote.node,
                remote.input,
                false,
            ) {
                self.edits.push(QueuedNodeEdit::Atomic(edit));
            }
        }
        snarl.drop_outputs(pin.id);
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<GraphItem>) {
        for remote in &pin.remotes {
            if let Some(edit) = edit_for_wire(
                self.project,
                snarl,
                remote.node,
                remote.output,
                pin.id.node,
                pin.id.input,
                false,
            ) {
                self.edits.push(QueuedNodeEdit::Atomic(edit));
            }
        }
        snarl.drop_inputs(pin.id);
    }

    fn draw_background(
        &mut self,
        _background: Option<&BackgroundPattern>,
        viewport: &egui::Rect,
        snarl_style: &SnarlStyle,
        style: &egui::Style,
        painter: &egui::Painter,
        _snarl: &Snarl<GraphItem>,
    ) {
        // `painter.clip_rect()` is the final Snarl viewport in graph space.
        // Preserve its screen-space equivalent for foreground painting, QA
        // geometry and coordinate interactions registered after `show`.
        *self.canvas_clip = *self.to_global * painter.clip_rect();
        let scale = sanitized_node_editor_scale(self.to_global.scaling);
        let mut grid_style = *snarl_style;
        grid_style.bg_pattern_stroke = Some(egui::Stroke::new(
            screen_stroke_in_graph_units(0.7, scale),
            Color32::from_rgba_premultiplied(115, 128, 152, 34),
        ));
        BackgroundPattern::grid(egui::Vec2::splat(adaptive_grid_spacing(scale)), 0.0).draw(
            viewport,
            &grid_style,
            style,
            painter,
        );

        for container in self.containers {
            paint_container_backdrop(
                painter,
                container,
                container_inactive(self.project, container.owner, self.current_time),
            );
        }
    }

    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<GraphItem>,
    ) {
        resolve_node_editor_transform(to_global, self.locked_canvas_transform);
        *self.to_global = *to_global;
    }
}

fn resolve_node_editor_transform(
    transform: &mut egui::emath::TSTransform,
    locked: Option<egui::emath::TSTransform>,
) {
    if let Some(locked) = locked {
        *transform = locked;
    }
    sanitize_node_editor_transform(transform);
}

impl ProjectNodeViewer<'_> {
    fn show_merge_layers(&mut self, merge_id: Uuid, ui: &mut egui::Ui) {
        let rows = merge_layer_rows(self.project, merge_id);
        let to_global = *self.to_global;
        let canvas_clip = *self.canvas_clip;
        let header = non_selectable_label(
            ui,
            egui::RichText::new("Layers · Back → Front")
                .small()
                .strong(),
        )
        .on_hover_text("Each row is one authored Merge input wire.");
        register_merge_layer_component(
            format!("node_editor.merge_layers.header:{merge_id}"),
            "node_editor_merge_layers_header",
            header.rect,
            true,
            to_global,
            canvas_clip,
            serde_json::json!({
                "merge_id": merge_id,
                "layer_count": rows.len(),
                "order_semantics": "back_to_front",
                "blend_ownership": "connection",
            }),
        );

        if rows.is_empty() {
            let empty = non_selectable_label(
                ui,
                egui::RichText::new("No image inputs")
                    .small()
                    .color(Color32::from_gray(135)),
            );
            register_merge_layer_component(
                format!("node_editor.merge_layers.empty:{merge_id}"),
                "node_editor_merge_layers_empty",
                empty.rect,
                false,
                to_global,
                canvas_clip,
                serde_json::json!({
                    "merge_id": merge_id,
                    "layer_count": 0,
                    "order_semantics": "back_to_front",
                }),
            );
            return;
        }

        for row in rows {
            let mut selected_blend = None;
            let mut requested_order = None;
            let row_response = egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(7, 5))
                .corner_radius(5)
                .fill(Color32::from_black_alpha(32))
                .show(ui, |ui| {
                    ui.set_width(216.0);
                    ui.horizontal(|ui| {
                        non_selectable_label(
                            ui,
                            egui::RichText::new(format!(
                                "{} / {}",
                                row.back_to_front_index + 1,
                                row.layer_count
                            ))
                            .small()
                            .strong(),
                        )
                        .on_hover_text("Layer position in Back → Front order");
                        bounded_non_selectable_label(
                            ui,
                            row.source_label.clone(),
                            158.0,
                            egui::Align::LEFT,
                        )
                        .on_hover_text(format!(
                            "{} output · {}",
                            row.source_label, row.source.port
                        ));
                    });

                    ui.horizontal(|ui| {
                        let combo = ui.add_enabled_ui(row.authored_blend_available, |ui| {
                            egui::ComboBox::from_id_salt((
                                "merge_layer_authored_blend",
                                merge_id,
                                row.connection_id,
                            ))
                            .selected_text(format!(
                                "Wire · {}",
                                blend_mode_label(row.authored_blend_mode)
                            ))
                            .width(178.0)
                            .show_ui(ui, |ui| {
                                for blend_mode in AUTHORED_BLEND_MODES {
                                    let selected = blend_mode == row.authored_blend_mode;
                                    let option = ui
                                        .add_enabled(
                                            !selected,
                                            egui::Button::selectable(
                                                selected,
                                                blend_mode_label(blend_mode),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text(
                                            "Authored on this input wire, not on the Merge Node.",
                                        );
                                    register_merge_layer_popup_component(
                                        format!(
                                            "node_editor.merge_layer.blend.{}:{merge_id}:{}",
                                            blend_mode_qa_key(blend_mode),
                                            row.connection_id
                                        ),
                                        "node_editor_merge_layer_blend_option",
                                        option.rect,
                                        option.enabled(),
                                        ui.clip_rect(),
                                        row.qa_metadata(Some(serde_json::json!({
                                            "action": "set_authored_blend",
                                            "blend_mode": blend_mode_qa_key(blend_mode),
                                            "selected": selected,
                                        }))),
                                    );
                                    if option.clicked() {
                                        selected_blend = Some(blend_mode);
                                        ui.close();
                                    }
                                }
                            })
                            .response
                        });
                        let combo_response = combo.inner;
                        register_merge_layer_component(
                            format!(
                                "node_editor.merge_layer.blend_select:{merge_id}:{}",
                                row.connection_id
                            ),
                            "node_editor_merge_layer_blend_select",
                            combo_response.rect,
                            combo_response.enabled(),
                            to_global,
                            canvas_clip,
                            row.qa_metadata(Some(serde_json::json!({
                                "action": "open_authored_blend",
                            }))),
                        );
                    });

                    ui.horizontal(|ui| {
                        let back_index = row.back_to_front_index.checked_sub(1);
                        let back = ui
                            .add_enabled(back_index.is_some(), egui::Button::new("← Back"))
                            .on_hover_text("Move this wire one layer toward the back");
                        register_merge_layer_component(
                            format!(
                                "node_editor.merge_layer.order_back:{merge_id}:{}",
                                row.connection_id
                            ),
                            "node_editor_merge_layer_order_button",
                            back.rect,
                            back.enabled(),
                            to_global,
                            canvas_clip,
                            row.qa_metadata(Some(serde_json::json!({
                                "action": "reorder",
                                "direction": "back",
                                "target_back_to_front_index": back_index,
                            }))),
                        );
                        if back.clicked() {
                            requested_order = back_index;
                        }

                        let front_index = (row.back_to_front_index + 1 < row.layer_count)
                            .then_some(row.back_to_front_index + 1);
                        let front = ui
                            .add_enabled(front_index.is_some(), egui::Button::new("Front →"))
                            .on_hover_text("Move this wire one layer toward the front");
                        register_merge_layer_component(
                            format!(
                                "node_editor.merge_layer.order_front:{merge_id}:{}",
                                row.connection_id
                            ),
                            "node_editor_merge_layer_order_button",
                            front.rect,
                            front.enabled(),
                            to_global,
                            canvas_clip,
                            row.qa_metadata(Some(serde_json::json!({
                                "action": "reorder",
                                "direction": "front",
                                "target_back_to_front_index": front_index,
                            }))),
                        );
                        if front.clicked() {
                            requested_order = front_index;
                        }
                    });
                })
                .response;

            register_merge_layer_component(
                format!("node_editor.merge_layer:{merge_id}:{}", row.connection_id),
                "node_editor_merge_layer",
                row_response.rect,
                true,
                to_global,
                canvas_clip,
                row.qa_metadata(None),
            );
            if let Some(blend_mode) = selected_blend {
                self.edits
                    .push(QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                        connection_id: row.connection_id,
                        blend_mode,
                    }));
            }
            if let Some(new_order) = requested_order {
                self.edits
                    .push(QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                        connection_id: row.connection_id,
                        new_order: new_order as i64,
                    }));
            }
        }

        let runtime_note = non_selectable_label(
            ui,
            egui::RichText::new(
                "Runtime: first produced layer composites as Normal; authored wire modes remain.",
            )
            .small()
            .weak(),
        );
        register_merge_layer_component(
            format!("node_editor.merge_layers.runtime_note:{merge_id}"),
            "node_editor_merge_layers_runtime_note",
            runtime_note.rect,
            false,
            to_global,
            canvas_clip,
            serde_json::json!({
                "merge_id": merge_id,
                "runtime_first_produced_may_be_normal": true,
                "authored_blend_ownership": "connection",
            }),
        );
    }

    fn queue_continuous_edit(
        &mut self,
        owner: PortOwner,
        key: impl Into<String>,
        edit: Option<NodeEdit>,
        finished: bool,
    ) {
        if edit.is_none() && !finished {
            return;
        }
        self.edits.push(QueuedNodeEdit::Continuous {
            pending: NodeEditorPendingEdit {
                owner,
                key: key.into(),
            },
            edit,
            finished,
        });
    }

    fn show_node_input_row(
        &mut self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        definition: &PinDefinition,
        property_key: &str,
        property_definition: Option<&PropertyDefinition>,
        connected: bool,
    ) {
        let property_time = node_property_time(self.project, node_id, self.current_time);
        let evaluated = self
            .project
            .get_node(node_id)
            .and_then(|node| node.properties().get(property_key))
            .map(|property| {
                evaluate_node_property(
                    self.project,
                    self.plugin_manager,
                    node_id,
                    property,
                    property_time,
                )
            });
        let value = evaluated
            .as_ref()
            .and_then(|evaluated| evaluated.value().cloned());
        let current_value_metadata = value
            .as_ref()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        let row = ui.horizontal(|ui| {
            bounded_non_selectable_label(ui, definition.name.clone(), 72.0, egui::Align::LEFT);
            if connected {
                non_selectable_label(
                    ui,
                    egui::RichText::new("linked")
                        .small()
                        .color(Color32::from_gray(145)),
                );
                return None;
            }
            if let Some(issue) = evaluated.as_ref().and_then(|evaluated| evaluated.issue()) {
                render_node_property_issue(ui, node_id, property_key, issue);
            }
            let Some(mut value) = value else {
                non_selectable_label(
                    ui,
                    egui::RichText::new("—")
                        .small()
                        .color(Color32::from_gray(105)),
                );
                return None;
            };
            let (changed, continuous, finished, control_kind, response) = match &mut value {
                PropertyValue::Number(number) => {
                    let response = if let Some(config) =
                        property_definition.and_then(FloatDragValueConfig::from_definition)
                    {
                        ui.add_sized([74.0, PORT_ROW_HEIGHT - 2.0], config.widget(&mut number.0))
                    } else {
                        ui.add_sized(
                            [74.0, PORT_ROW_HEIGHT - 2.0],
                            egui::DragValue::new(&mut number.0).speed(0.05),
                        )
                    };
                    (
                        response.changed(),
                        true,
                        continuous_response_finished(ui, &response),
                        "float",
                        response,
                    )
                }
                PropertyValue::Integer(integer) => {
                    let config = property_definition.and_then(|definition| {
                        IntegerDragValueConfig::from_ui_type(definition.ui_type())
                    });
                    let response = if let Some(config) = config {
                        ui.add_sized([74.0, PORT_ROW_HEIGHT - 2.0], config.widget(integer))
                    } else {
                        ui.add_sized([74.0, PORT_ROW_HEIGHT - 2.0], egui::DragValue::new(integer))
                    };
                    (
                        response.changed(),
                        true,
                        continuous_response_finished(ui, &response),
                        "integer",
                        response,
                    )
                }
                PropertyValue::String(text) => {
                    if let Some(PropertyUiType::Dropdown { options }) =
                        property_definition.map(PropertyDefinition::ui_type)
                    {
                        let before = text.clone();
                        let response = egui::ComboBox::from_id_salt((node_id, property_key))
                            .selected_text(text.as_str())
                            .width(96.0)
                            .show_ui(ui, |ui| {
                                for option in options {
                                    ui.selectable_value(text, option.clone(), option);
                                }
                            })
                            .response;
                        (
                            before != *text,
                            false,
                            response.lost_focus(),
                            "dropdown",
                            response,
                        )
                    } else {
                        let response = ui.add_sized(
                            [96.0, PORT_ROW_HEIGHT - 2.0],
                            egui::TextEdit::singleline(text).clip_text(true),
                        );
                        (
                            response.changed(),
                            true,
                            continuous_response_finished(ui, &response),
                            "text",
                            response,
                        )
                    }
                }
                PropertyValue::Boolean(boolean) => {
                    let response = ui.checkbox(boolean, "");
                    (response.changed(), false, false, "boolean", response)
                }
                PropertyValue::Color(color) => {
                    let mut edited =
                        Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
                    let (response, popup_closed) = continuous_color_edit_button(ui, &mut edited);
                    let changed = response.changed();
                    if changed {
                        color.r = edited.r();
                        color.g = edited.g();
                        color.b = edited.b();
                        color.a = edited.a();
                    }
                    (
                        changed,
                        true,
                        popup_closed || continuous_response_finished(ui, &response),
                        "color",
                        response,
                    )
                }
                PropertyValue::Vec2(vec) => {
                    let response = non_selectable_label(
                        ui,
                        format!("{:.1}, {:.1}", vec.x.into_inner(), vec.y.into_inner()),
                    );
                    (false, false, false, "vec2_readonly", response)
                }
                PropertyValue::Vec3(_)
                | PropertyValue::Vec4(_)
                | PropertyValue::Array(_)
                | PropertyValue::Map(_) => {
                    let response = non_selectable_label(
                        ui,
                        egui::RichText::new("complex")
                            .small()
                            .color(Color32::from_gray(125)),
                    );
                    (false, false, false, "complex_readonly", response)
                }
            };
            let qa_value = serde_json::Value::from(&value);
            let edit = changed.then(|| NodeEdit::SetProperty {
                owner: PortOwner::Node(node_id),
                key: property_key.to_string(),
                time: property_time,
                value,
            });
            if continuous {
                self.queue_continuous_edit(PortOwner::Node(node_id), property_key, edit, finished);
            } else if let Some(edit) = edit {
                self.edits.push(QueuedNodeEdit::Atomic(edit));
            }
            Some((response, control_kind, qa_value))
        });
        let (response, control_kind, enabled, value) = match row.inner {
            Some((response, control_kind, value)) => {
                let enabled = response.enabled();
                (response, control_kind, enabled, value)
            }
            None => (
                row.response,
                if connected { "linked" } else { "missing" },
                false,
                current_value_metadata,
            ),
        };
        let component_id = format!("node_editor.property.node:{node_id}:{property_key}");
        let unclipped_rect = *self.to_global * response.rect;
        let rect = clipped_qa_rect(unclipped_rect, *self.canvas_clip);
        #[cfg(test)]
        capture_test_rect(&component_id, rect);
        let operation_identity = self.project.get_node(node_id).and_then(|node| {
            let NodeContent::PluginOperation(operation) = node.content() else {
                return None;
            };
            Some(serde_json::json!({
                "category": operation.category,
                "component_id": operation.component_id,
                "operation": operation.operation,
            }))
        });
        crate::qa::register_component_with_metadata(
            component_id,
            "node_property_control",
            rect,
            enabled,
            Some(serde_json::json!({
                "node_id": node_id,
                "property": property_key,
                "port": definition.key,
                "connected": connected,
                "control_kind": control_kind,
                "current_time": property_time,
                "value": value,
                "operation_identity": operation_identity,
                "descriptor_available": property_definition.is_some(),
                "definition": property_definition.map(
                    crate::ui::panels::inspector::properties::property_definition_metadata
                ),
                "unclipped_rect": qa_rect_metadata(unclipped_rect),
                "visible_in_canvas": rect.is_positive(),
            })),
        );
    }

    fn show_container_body(&mut self, owner: PortOwner, ui: &mut egui::Ui) {
        let Some((mut name, mut size)) = container_name_and_size(self.project, owner) else {
            return;
        };

        ui.horizontal(|ui| {
            property_label(ui, "Name");
            let response = ui.add_sized(
                [180.0, PORT_ROW_HEIGHT],
                egui::TextEdit::singleline(&mut name),
            );
            let finished = continuous_response_finished(ui, &response);
            let edit = response
                .changed()
                .then_some(NodeEdit::RenameContainer { owner, name });
            self.queue_continuous_edit(owner, "$name", edit, finished);
        });

        if let PortOwner::Clip(clip_id) = owner {
            let Some(clip) = self.project.get_clip(clip_id) else {
                return;
            };
            let timing_controls = Clip::timing_property_definitions()
                .iter()
                .filter_map(|definition| {
                    clip.timing_property_value(definition.name())
                        .and_then(|value| value.get_as::<f64>())
                        .map(|value| (definition, value))
                })
                .collect::<Vec<_>>();
            for (definition, value) in timing_controls {
                let mut edited = value;
                ui.horizontal(|ui| {
                    property_label(ui, definition.label());
                    let Some(config) = node_timing_drag_config(definition) else {
                        log::error!(
                            "Clip timing property {} is missing Float drag metadata",
                            definition.name()
                        );
                        return;
                    };
                    let response = ui.add_sized(
                        [INLINE_CONTROL_WIDTH, PORT_ROW_HEIGHT],
                        config.widget(&mut edited),
                    );
                    let finished = continuous_response_finished(ui, &response);
                    let edit = response.changed().then(|| NodeEdit::SetProperty {
                        owner,
                        key: definition.name().to_string(),
                        time: self.current_time,
                        value: PropertyValue::Number(OrderedFloat(edited)),
                    });
                    self.queue_continuous_edit(owner, definition.name(), edit, finished);
                });
            }
        }
        ui.horizontal(|ui| {
            property_label(ui, "Size");
            let width_response = ui.add(
                egui::DragValue::new(&mut size[0])
                    .speed(1.0)
                    .range(MIN_CONTAINER_SIZE.x..=8192.0)
                    .suffix(" w"),
            );
            let height_response = ui.add(
                egui::DragValue::new(&mut size[1])
                    .speed(1.0)
                    .range(MIN_CONTAINER_SIZE.y..=8192.0)
                    .suffix(" h"),
            );
            let resized = || NodeEdit::ResizeContainer {
                owner,
                size: [
                    size[0].max(MIN_CONTAINER_SIZE.x),
                    size[1].max(MIN_CONTAINER_SIZE.y),
                ],
            };
            self.queue_continuous_edit(
                owner,
                "$size.width",
                width_response.changed().then(&resized),
                continuous_response_finished(ui, &width_response),
            );
            self.queue_continuous_edit(
                owner,
                "$size.height",
                height_response.changed().then(resized),
                continuous_response_finished(ui, &height_response),
            );
        });
    }

    fn edit_string_property(
        &mut self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        node: &Node,
        key: &str,
        label: &str,
        fallback: &str,
    ) {
        let property_time = node_property_time(self.project, node_id, self.current_time);
        let evaluated = node.properties().get(key).map(|property| {
            evaluate_node_property(
                self.project,
                self.plugin_manager,
                node_id,
                property,
                property_time,
            )
        });
        let mut value = evaluated
            .as_ref()
            .and_then(|evaluated| evaluated.value())
            .and_then(|value| value.get_as::<String>())
            .unwrap_or_else(|| fallback.to_string());
        ui.horizontal(|ui| {
            property_label(ui, label);
            if let Some(issue) = evaluated.as_ref().and_then(|evaluated| evaluated.issue()) {
                render_node_property_issue(ui, node_id, key, issue);
            }
            let response = ui.add_sized(
                [INLINE_CONTROL_WIDTH, PORT_ROW_HEIGHT],
                egui::TextEdit::singleline(&mut value),
            );
            let finished = continuous_response_finished(ui, &response);
            let edit = response.changed().then(|| NodeEdit::SetProperty {
                owner: PortOwner::Node(node_id),
                key: key.to_string(),
                time: property_time,
                value: PropertyValue::String(value),
            });
            self.queue_continuous_edit(PortOwner::Node(node_id), key.to_string(), edit, finished);
        });
    }
}

fn continuous_response_finished(ui: &egui::Ui, response: &egui::Response) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
}

fn continuous_color_edit_button(ui: &mut egui::Ui, color: &mut Color32) -> (egui::Response, bool) {
    // `color_edit_button_srgba` derives its popup id from the current auto id
    // with the same salt. Observe that public popup state so closing the
    // picker becomes the history commit boundary, even on a frame where the
    // color itself no longer changes.
    let popup_id = ui.auto_id_with("popup");
    let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let response = ui.color_edit_button_srgba(color);
    let closed = was_open && !egui::Popup::is_id_open(ui.ctx(), popup_id);
    (response, closed)
}

fn non_selectable_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Label::new(text).selectable(false))
}

fn property_label(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    bounded_non_selectable_label(ui, text, PROPERTY_LABEL_WIDTH, egui::Align::LEFT)
}

fn bounded_non_selectable_label(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    width: f32,
    align: egui::Align,
) -> egui::Response {
    ui.add_sized(
        [width, PORT_ROW_HEIGHT],
        egui::Label::new(text.into())
            .selectable(false)
            .truncate()
            .halign(align),
    )
}

fn port_label_width(_item: Option<GraphItem>) -> f32 {
    PORT_LABEL_WIDTH
}

fn strong_non_selectable_label(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(text.into()).strong()).selectable(false))
}

fn bounded_strong_non_selectable_label(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    width: f32,
) -> egui::Response {
    ui.add_sized(
        [width, PORT_ROW_HEIGHT],
        egui::Label::new(egui::RichText::new(text.into()).strong())
            .selectable(false)
            .truncate(),
    )
}

#[derive(Clone, Copy)]
struct NodePalette {
    body: Color32,
    header: Color32,
    accent: Color32,
}

const VALUE_NODE_CATEGORY_LABEL: &str = "Value";

fn value_operation_label(value: library::model::ValueContent) -> &'static str {
    match value {
        library::model::ValueContent::TimeModulo => "Time Modulo",
    }
}

fn node_palette(project: &Project, node_id: Uuid) -> NodePalette {
    match project.get_node(node_id).map(Node::content) {
        Some(NodeContent::Generator(GeneratorContent::Text)) => NodePalette {
            body: Color32::from_rgb(32, 36, 48),
            header: Color32::from_rgb(77, 57, 112),
            accent: Color32::from_rgb(190, 146, 245),
        },
        Some(NodeContent::Generator(GeneratorContent::Shape)) => NodePalette {
            body: Color32::from_rgb(30, 41, 42),
            header: Color32::from_rgb(42, 91, 82),
            accent: Color32::from_rgb(94, 213, 172),
        },
        Some(NodeContent::Generator(GeneratorContent::Solid)) => NodePalette {
            body: Color32::from_rgb(42, 38, 31),
            header: Color32::from_rgb(103, 76, 38),
            accent: Color32::from_rgb(238, 190, 89),
        },
        Some(NodeContent::Generator(GeneratorContent::SkSL)) => NodePalette {
            body: Color32::from_rgb(28, 41, 48),
            header: Color32::from_rgb(38, 86, 105),
            accent: Color32::from_rgb(92, 199, 226),
        },
        Some(NodeContent::Media(_)) => NodePalette {
            body: Color32::from_rgb(32, 39, 50),
            header: Color32::from_rgb(45, 77, 117),
            accent: Color32::from_rgb(100, 170, 243),
        },
        Some(NodeContent::Reference(_)) => NodePalette {
            body: Color32::from_rgb(38, 36, 49),
            header: Color32::from_rgb(74, 63, 111),
            accent: Color32::from_rgb(162, 139, 232),
        },
        Some(NodeContent::PluginOperation(_)) => NodePalette {
            body: Color32::from_rgb(42, 34, 49),
            header: Color32::from_rgb(91, 54, 112),
            accent: Color32::from_rgb(205, 139, 232),
        },
        Some(NodeContent::Value(_)) => NodePalette {
            body: Color32::from_rgb(28, 41, 46),
            header: Color32::from_rgb(39, 83, 95),
            accent: Color32::from_rgb(91, 197, 218),
        },
        Some(NodeContent::Merge) | None => NodePalette {
            body: Color32::from_rgb(38, 39, 43),
            header: Color32::from_rgb(68, 70, 79),
            accent: Color32::from_rgb(177, 182, 198),
        },
    }
}

fn node_icon(project: &Project, node_id: Uuid) -> &'static str {
    match project.get_node(node_id).map(Node::content) {
        Some(NodeContent::Generator(GeneratorContent::Text)) => "T",
        Some(NodeContent::Generator(GeneratorContent::Shape)) => "◇",
        Some(NodeContent::Generator(GeneratorContent::Solid)) => "■",
        Some(NodeContent::Generator(GeneratorContent::SkSL)) => "ƒ",
        Some(NodeContent::Media(_)) => "▶",
        Some(NodeContent::Reference(_)) => "↗",
        Some(NodeContent::PluginOperation(operation)) => match operation.category.as_str() {
            "style" => "◐",
            "effect" => "✦",
            "effector" => "↯",
            "decorator" => "⌁",
            _ => "P",
        },
        Some(NodeContent::Value(_)) => "%",
        Some(NodeContent::Merge) => "⋈",
        None => "?",
    }
}

fn container_icon(owner: PortOwner) -> &'static str {
    match owner {
        PortOwner::Composition(_) => "◉",
        PortOwner::Track(_) => "≡",
        PortOwner::Clip(_) => "▱",
        PortOwner::Node(_) => "●",
    }
}

fn paint_container_backdrop(painter: &egui::Painter, container: &ContainerVisual, inactive: bool) {
    let rect = container.rect();
    let mut fill = match container.kind {
        ContainerKind::Composition => Color32::from_rgba_premultiplied(25, 43, 67, 70),
        ContainerKind::Track => Color32::from_rgba_premultiplied(48, 43, 61, 64),
        ContainerKind::Clip => Color32::from_rgba_premultiplied(38, 60, 47, 66),
    };
    if inactive {
        fill = fill.gamma_multiply(0.35);
    }
    let radius = egui::CornerRadius::same(8);
    painter.rect_filled(rect, radius, fill);
    let mut header_fill = match container.kind {
        ContainerKind::Composition => Color32::from_rgba_premultiplied(38, 66, 100, 220),
        ContainerKind::Track => Color32::from_rgba_premultiplied(73, 61, 91, 220),
        ContainerKind::Clip => Color32::from_rgba_premultiplied(52, 88, 64, 220),
    };
    if inactive {
        header_fill = header_fill.gamma_multiply(0.42);
    }
    let header = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), CONTAINER_HEADER_HEIGHT.min(rect.height())),
    );
    painter.rect_filled(
        header,
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 2,
            se: 2,
        },
        header_fill,
    );
}

fn paint_container_foreground(
    painter: &egui::Painter,
    project: &Project,
    container: &ContainerVisual,
    inactive: bool,
    scale: f32,
) {
    let rect = container.rect();
    let detailed = node_editor_details_visible(scale);
    let mut stroke = match container.kind {
        ContainerKind::Composition => egui::Stroke::new(
            if detailed {
                2.0
            } else {
                screen_stroke_in_graph_units(1.4, scale)
            },
            Color32::from_rgb(74, 137, 207),
        ),
        ContainerKind::Track => egui::Stroke::new(
            if detailed {
                1.5
            } else {
                screen_stroke_in_graph_units(1.15, scale)
            },
            Color32::from_rgb(143, 116, 196),
        ),
        ContainerKind::Clip => egui::Stroke::new(
            if detailed {
                1.5
            } else {
                screen_stroke_in_graph_units(1.15, scale)
            },
            Color32::from_rgb(95, 174, 121),
        ),
    };
    if inactive {
        stroke.color = stroke.color.gamma_multiply(0.5);
    }
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        stroke,
        egui::StrokeKind::Inside,
    );
    if !detailed {
        return;
    }
    let header_bottom = rect.top() + CONTAINER_HEADER_HEIGHT.min(rect.height());
    painter.line_segment(
        [
            egui::pos2(rect.left(), header_bottom),
            egui::pos2(rect.right(), header_bottom),
        ],
        egui::Stroke::new(1.0, stroke.color.gamma_multiply(0.82)),
    );
    painter.text(
        rect.right_top() + egui::vec2(-12.0, 10.0),
        egui::Align2::RIGHT_TOP,
        match container.kind {
            ContainerKind::Composition => "COMPOSITION",
            ContainerKind::Track => "TRACK",
            ContainerKind::Clip => "CLIP",
        },
        egui::FontId::proportional(11.0),
        Color32::from_white_alpha(155),
    );

    if !container.collapsed {
        for (index, definition) in canonical_pin_definitions(
            project,
            container.owner,
            PortDirection::Output,
            PortSide::Left,
        )
        .iter()
        .enumerate()
        {
            painter.text(
                egui::pos2(
                    rect.left() + EMBEDDED_PORT_LABEL_INSET,
                    rect.top() + CONTAINER_PORT_Y + index as f32 * PORT_ROW_HEIGHT,
                ),
                egui::Align2::LEFT_CENTER,
                &definition.name,
                egui::FontId::proportional(11.0),
                pin_color(definition.data_type).gamma_multiply(if inactive { 0.45 } else { 0.9 }),
            );
        }
        painter.text(
            egui::pos2(
                rect.right() - EMBEDDED_PORT_LABEL_INSET,
                rect.top() + CONTAINER_HEADER_HEIGHT * 0.5,
            ),
            egui::Align2::RIGHT_CENTER,
            "IMAGE OUT",
            egui::FontId::proportional(10.0),
            pin_color(PortDataType::Image).gamma_multiply(if inactive { 0.45 } else { 0.9 }),
        );
    }

    if let PortOwner::Clip(clip_id) = container.owner {
        if let Some(clip) = project.get_clip(clip_id) {
            painter.text(
                rect.right_top() + egui::vec2(-12.0, 35.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "{:.2}s  ·  {:.2}s  ·  ×{:.2}",
                    clip.start_time.into_inner(),
                    clip.duration.into_inner(),
                    clip.time_stretch.into_inner()
                ),
                egui::FontId::proportional(10.0),
                Color32::from_white_alpha(if inactive { 75 } else { 135 }),
            );
        }
    }
}

fn pin_color(data_type: PortDataType) -> Color32 {
    match data_type {
        PortDataType::Image => Color32::from_rgb(238, 207, 109),
        PortDataType::Shape => Color32::from_rgb(142, 132, 246),
        PortDataType::Audio => Color32::from_rgb(100, 200, 100),
        PortDataType::String => Color32::from_rgb(100, 220, 220),
        PortDataType::Path => Color32::from_rgb(100, 150, 255),
        PortDataType::Number | PortDataType::Integer => Color32::from_rgb(255, 100, 100),
        PortDataType::Color => Color32::from_rgb(220, 120, 220),
        PortDataType::Vec2 => Color32::from_rgb(120, 170, 255),
        PortDataType::Vec3 => Color32::from_rgb(105, 195, 235),
        PortDataType::Vec4 => Color32::from_rgb(145, 145, 245),
        PortDataType::Boolean => Color32::from_rgb(220, 160, 100),
        PortDataType::Any => Color32::from_rgb(200, 200, 200),
    }
}

fn pin_info(data_type: PortDataType, connected: bool) -> PinInfo {
    let color = pin_color(data_type);
    let fill = if connected {
        color
    } else {
        color.gamma_multiply(0.32)
    };
    PinInfo::circle()
        .with_fill(fill)
        .with_stroke(egui::Stroke::new(if connected { 2.0 } else { 1.25 }, color))
        .with_wire_color(color)
        .with_wire_style(WireStyle::Bezier3)
}

fn qa_container_key(owner: PortOwner) -> String {
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
fn clipped_qa_rect(rect: egui::Rect, canvas_clip: egui::Rect) -> egui::Rect {
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

fn wire_port_drop_rect(rendered_port_rect: egui::Rect) -> egui::Rect {
    rendered_port_rect.expand(WIRE_PORT_DROP_RADIUS)
}

fn qa_rect_metadata(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min_x": rect.min.x,
        "min_y": rect.min.y,
        "max_x": rect.max.x,
        "max_y": rect.max.y,
        "width": rect.width(),
        "height": rect.height(),
    })
}

fn edge_endpoint_qa_metadata(
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

fn qa_port_id(
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
                PortAnchorKind::ExternalImage => "external_output",
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

fn node_title(project: &Project, node_id: Uuid) -> String {
    project
        .get_node(node_id)
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "Missing node".to_string())
}

fn graph_item_title(project: &Project, item: GraphItem) -> String {
    match item {
        GraphItem::Node(node_id) => node_title(project, node_id),
        GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
            container_title(project, owner)
        }
    }
}

fn container_title(project: &Project, owner: PortOwner) -> String {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| format!("Composition · {}", composition.name))
            .unwrap_or_else(|| "Missing Composition".to_string()),
        PortOwner::Track(id) => project
            .get_track(id)
            .map(|track| format!("Track · {}", track.name))
            .unwrap_or_else(|| "Missing Track".to_string()),
        PortOwner::Clip(id) => project
            .get_clip(id)
            .map(|clip| format!("Clip · {}", clip.name))
            .unwrap_or_else(|| "Missing Clip".to_string()),
        PortOwner::Node(id) => project
            .get_node(id)
            .map(|node| format!("Node · {}", node.name))
            .unwrap_or_else(|| "Missing Node".to_string()),
    }
}

fn container_name_and_size(project: &Project, owner: PortOwner) -> Option<(String, [f32; 2])> {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| (composition.name.clone(), composition.ui_size)),
        PortOwner::Track(id) => project
            .get_track(id)
            .map(|track| (track.name.clone(), track.ui_size)),
        PortOwner::Clip(id) => project
            .get_clip(id)
            .map(|clip| (clip.name.clone(), clip.ui_size)),
        PortOwner::Node(_) => None,
    }
}

fn container_collapsed(project: &Project, owner: PortOwner) -> Option<bool> {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.ui_collapsed),
        PortOwner::Track(id) => project.get_track(id).map(|track| track.ui_collapsed),
        PortOwner::Clip(id) => project.get_clip(id).map(|clip| clip.ui_collapsed),
        PortOwner::Node(_) => None,
    }
}

fn container_output_node_id(project: &Project, owner: PortOwner) -> Option<Uuid> {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .and_then(|composition| composition.output_node_id),
        PortOwner::Track(id) => project.get_track(id).and_then(|track| track.output_node_id),
        PortOwner::Clip(id) => project.get_clip(id).and_then(|clip| clip.output_node_id),
        PortOwner::Node(_) => None,
    }
}

/// Node properties share the evaluator's time domain. A Node directly owned
/// by a Clip is evaluated and edited in that Clip's source-local time; Nodes
/// owned directly by a Track or Composition stay in global composition time.
fn node_property_time(project: &Project, node_id: Uuid, global_time: f64) -> f64 {
    project
        .find_parent_clip(node_id)
        .and_then(|clip_id| project.get_clip(clip_id))
        .map_or(global_time, |clip| clip.local_time(global_time))
}

fn plugin_operation_property_definition(
    plugin_manager: &PluginManager,
    node: &Node,
    property_name: &str,
) -> Option<PropertyDefinition> {
    let NodeContent::PluginOperation(operation) = node.content() else {
        return None;
    };
    plugin_manager
        .operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        )
        .ok()?
        .properties()
        .iter()
        .find(|definition| definition.name() == property_name)
        .cloned()
}

pub(super) fn node_timing_drag_config(
    definition: &library::model::property::PropertyDefinition,
) -> Option<FloatDragValueConfig> {
    FloatDragValueConfig::from_definition(definition)
}

fn clip_is_active(clip: &library::model::Clip, current_time: f64) -> bool {
    current_time >= clip.start_time.into_inner() && current_time < clip.end_time()
}

fn container_inactive(project: &Project, owner: PortOwner, current_time: f64) -> bool {
    match owner {
        PortOwner::Clip(id) => project
            .get_clip(id)
            .is_some_and(|clip| !clip_is_active(clip, current_time)),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphItemInactiveReason {
    Disabled,
    OutsideClipRange,
}

impl GraphItemInactiveReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::OutsideClipRange => "outside_clip_range",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Disabled => "No output (Node disabled)",
            Self::OutsideClipRange => "No output (outside Clip range)",
        }
    }
}

fn graph_item_inactive_reason(
    project: &Project,
    item: GraphItem,
    current_time: f64,
) -> Option<GraphItemInactiveReason> {
    match item {
        GraphItem::Node(node_id) => {
            if project.get_node(node_id).is_some_and(|node| !node.enabled) {
                return Some(GraphItemInactiveReason::Disabled);
            }
            project
                .find_parent_clip(node_id)
                .and_then(|clip_id| project.get_clip(clip_id))
                .filter(|clip| !clip_is_active(clip, current_time))
                .map(|_| GraphItemInactiveReason::OutsideClipRange)
        }
        GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
            container_inactive(project, owner, current_time)
                .then_some(GraphItemInactiveReason::OutsideClipRange)
        }
    }
}

fn graph_item_inactive(project: &Project, item: GraphItem, current_time: f64) -> bool {
    graph_item_inactive_reason(project, item, current_time).is_some()
}

fn input_definitions(project: &Project, item: GraphItem) -> Vec<PinDefinition> {
    let owner = match item {
        GraphItem::Node(node_id) => PortOwner::Node(node_id),
        GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::ExternalInputs,
        } => owner,
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ImageSink,
            ..
        } => {
            return vec![PinDefinition {
                key: "output_binding".to_string(),
                name: "Image".to_string(),
                data_type: PortDataType::Image,
            }];
        }
        GraphItem::Container(_) | GraphItem::PortAnchor { .. } => return Vec::new(),
    };
    canonical_pin_definitions(project, owner, PortDirection::Input, PortSide::Left)
}

fn output_definitions(project: &Project, item: GraphItem) -> Vec<PinDefinition> {
    let (owner, side) = match item {
        GraphItem::Node(node_id) => (PortOwner::Node(node_id), PortSide::Right),
        GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::InternalMetadata,
        } => (owner, PortSide::Left),
        GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::ExternalImage,
        } => (owner, PortSide::Right),
        GraphItem::Container(_) | GraphItem::PortAnchor { .. } => return Vec::new(),
    };
    canonical_pin_definitions(project, owner, PortDirection::Output, side)
}

fn canonical_pin_definitions(
    project: &Project,
    owner: PortOwner,
    direction: PortDirection,
    side: PortSide,
) -> Vec<PinDefinition> {
    project
        .port_definitions(owner)
        .into_iter()
        .filter(|definition| definition.direction == direction && definition.side == side)
        .map(|definition| PinDefinition {
            key: definition.key,
            name: definition.label,
            data_type: definition.data_type,
        })
        .collect()
}

fn edit_for_wire(
    project: &Project,
    snarl: &Snarl<GraphItem>,
    source_snarl_id: egui_snarl::NodeId,
    output_index: usize,
    target_snarl_id: egui_snarl::NodeId,
    input_index: usize,
    connect: bool,
) -> Option<NodeEdit> {
    let source_item = *snarl.get_node(source_snarl_id)?;
    let target_item = *snarl.get_node(target_snarl_id)?;
    if let GraphItem::PortAnchor {
        owner,
        kind: PortAnchorKind::ImageSink,
    } = target_item
    {
        let GraphItem::Node(node_id) = source_item else {
            return None;
        };
        return Some(NodeEdit::SetOutputNode {
            owner,
            node_id: connect.then_some(node_id),
        });
    }
    let output_key = output_definitions(project, source_item)
        .get(output_index)?
        .key
        .clone();
    let input_key = input_definitions(project, target_item)
        .get(input_index)?
        .key
        .clone();
    let from = PortAddress::new(graph_item_owner(source_item)?, output_key);
    let to = PortAddress::new(graph_item_owner(target_item)?, input_key);

    if connect {
        Some(NodeEdit::Connect { from, to })
    } else {
        Some(NodeEdit::Disconnect { from, to })
    }
}

fn graph_item_owner(item: GraphItem) -> Option<PortOwner> {
    match item {
        GraphItem::Node(node_id) => Some(PortOwner::Node(node_id)),
        GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => Some(owner),
    }
}

fn embedded_pin_center(
    containers: &[ContainerVisual],
    item: Option<GraphItem>,
    _direction: PortDirection,
    index: usize,
) -> Option<egui::Pos2> {
    let GraphItem::PortAnchor { owner, kind } = item? else {
        return None;
    };
    let visual = containers
        .iter()
        .find(|container| container.owner == owner)?;
    let rect = visual.rect();
    let row_y = if visual.collapsed {
        rect.top() + 11.0 + index as f32 * 9.0
    } else {
        rect.top() + CONTAINER_PORT_Y + index as f32 * PORT_ROW_HEIGHT
    };
    Some(match kind {
        PortAnchorKind::ExternalInputs => egui::pos2(rect.left() - 7.0, row_y),
        PortAnchorKind::InternalMetadata => egui::pos2(rect.left() + 7.0, row_y),
        PortAnchorKind::ImageSink => egui::pos2(
            rect.right() - 7.0,
            rect.top() + CONTAINER_HEADER_HEIGHT * 0.5,
        ),
        PortAnchorKind::ExternalImage => {
            egui::pos2(rect.right(), rect.top() + CONTAINER_HEADER_HEIGHT * 0.5)
        }
    })
}

fn register_container_chrome(
    container: &ContainerVisual,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    project: &Project,
    current_time: f64,
) {
    let owner = qa_container_key(container.owner);
    let unclipped_main = to_global * container.rect();
    let main = clipped_qa_rect(unclipped_main, canvas_clip);
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
            "output_node_id": container_output_node_id(project, container.owner),
            "unclipped_rect": qa_rect_metadata(unclipped_main),
            "visible_in_canvas": main.is_positive(),
        })),
    );
}

fn register_rendered_edges(
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
        let sink = PortAddress::new(owner, "output_binding");
        for source in project.container_image_sources(owner) {
            let from = PortAddress::new(source.source, library::model::project::IMAGE_OUTPUT_PORT);
            let source_key = qa_container_key(source.source);
            let (id, kind) = match source.kind {
                ContainerImageSourceKind::OutputBinding => {
                    let PortOwner::Node(node_id) = source.source else {
                        continue;
                    };
                    (
                        format!(
                            "node_editor.edge.output_binding:{}:{node_id}",
                            qa_container_key(owner),
                        ),
                        RenderedEdgeKind::OutputBinding { owner, node_id },
                    )
                }
                ContainerImageSourceKind::DerivedChild => (
                    format!(
                        "node_editor.edge.derived:{}:{source_key}",
                        qa_container_key(owner)
                    ),
                    RenderedEdgeKind::DerivedOutput {
                        owner,
                        source: source.source,
                    },
                ),
            };
            if let Some(edge) = register_edge_component(
                EdgeComponent {
                    id,
                    kind,
                    from: &from,
                    to: &sink,
                    wire_color: pin_color(PortDataType::Image),
                    authored_order: None,
                    back_to_front_index: None,
                    layer_count: None,
                    authored_blend_mode: None,
                    authored_blend_available: false,
                },
                &ports,
                canvas_clip,
                overview,
            ) {
                rendered_edges.push(edge);
            }
        }
    }
    rendered_edges
}

fn register_edge_component(
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
    let (binding_owner, binding_node_id) = match edge.kind {
        RenderedEdgeKind::OutputBinding { owner, node_id } => {
            (Some(qa_container_key(owner)), Some(node_id))
        }
        _ => (None, None),
    };
    let (derived_owner, derived_source) = match edge.kind {
        RenderedEdgeKind::DerivedOutput { owner, source } => (
            Some(qa_container_key(owner)),
            Some(qa_container_key(source)),
        ),
        _ => (None, None),
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
            "derived_owner": derived_owner,
            "derived_source": derived_source,
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

fn cubic_bezier_point(
    start: egui::Pos2,
    control_a: egui::Pos2,
    control_b: egui::Pos2,
    end: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let one_minus_t = 1.0 - t;
    let weights = [
        one_minus_t.powi(3),
        3.0 * one_minus_t.powi(2) * t,
        3.0 * one_minus_t * t.powi(2),
        t.powi(3),
    ];
    egui::pos2(
        start.x * weights[0]
            + control_a.x * weights[1]
            + control_b.x * weights[2]
            + end.x * weights[3],
        start.y * weights[0]
            + control_a.y * weights[1]
            + control_b.y * weights[2]
            + end.y * weights[3],
    )
}

fn distance_to_segment(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn distance_to_rendered_edge(point: egui::Pos2, edge: &RenderedEdge) -> f32 {
    let mut previous = edge.start;
    let mut distance = f32::INFINITY;
    for sample in 1..=32 {
        let current = cubic_bezier_point(
            edge.start,
            edge.control_a,
            edge.control_b,
            edge.end,
            sample as f32 / 32.0,
        );
        distance = distance.min(distance_to_segment(point, previous, current));
        previous = current;
    }
    distance
}

fn segment_orientation(start: egui::Pos2, end: egui::Pos2, point: egui::Pos2) -> f32 {
    let segment = end - start;
    let offset = point - start;
    segment.x * offset.y - segment.y * offset.x
}

fn point_on_segment(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> bool {
    const EPSILON: f32 = 1.0e-4;
    segment_orientation(start, end, point).abs() <= EPSILON
        && point.x >= start.x.min(end.x) - EPSILON
        && point.x <= start.x.max(end.x) + EPSILON
        && point.y >= start.y.min(end.y) - EPSILON
        && point.y <= start.y.max(end.y) + EPSILON
}

fn segments_intersect(
    left_start: egui::Pos2,
    left_end: egui::Pos2,
    right_start: egui::Pos2,
    right_end: egui::Pos2,
) -> bool {
    let left_to_right_start = segment_orientation(left_start, left_end, right_start);
    let left_to_right_end = segment_orientation(left_start, left_end, right_end);
    let right_to_left_start = segment_orientation(right_start, right_end, left_start);
    let right_to_left_end = segment_orientation(right_start, right_end, left_end);
    let crosses_both_lines = left_to_right_start * left_to_right_end < 0.0
        && right_to_left_start * right_to_left_end < 0.0;
    if crosses_both_lines {
        return true;
    }

    const EPSILON: f32 = 1.0e-4;
    (left_to_right_start.abs() <= EPSILON && point_on_segment(right_start, left_start, left_end))
        || (left_to_right_end.abs() <= EPSILON && point_on_segment(right_end, left_start, left_end))
        || (right_to_left_start.abs() <= EPSILON
            && point_on_segment(left_start, right_start, right_end))
        || (right_to_left_end.abs() <= EPSILON
            && point_on_segment(left_end, right_start, right_end))
}

fn knife_segment_hits_edge(start: egui::Pos2, end: egui::Pos2, edge: &RenderedEdge) -> bool {
    let mut previous = edge.start;
    for sample in 1..=48 {
        let current = cubic_bezier_point(
            edge.start,
            edge.control_a,
            edge.control_b,
            edge.end,
            sample as f32 / 48.0,
        );
        let within_tolerance = segments_intersect(start, end, previous, current)
            || distance_to_segment(start, previous, current) <= 3.0
            || distance_to_segment(end, previous, current) <= 3.0
            || distance_to_segment(previous, start, end) <= 3.0
            || distance_to_segment(current, start, end) <= 3.0;
        if within_tolerance {
            return true;
        }
        previous = current;
    }
    false
}

fn rendered_edge_at_position(
    edges: &[RenderedEdge],
    position: egui::Pos2,
) -> Option<&RenderedEdge> {
    edges
        .iter()
        .filter_map(|edge| {
            let distance = distance_to_rendered_edge(position, edge);
            (distance <= WIRE_HIT_RADIUS).then_some((
                edge,
                edge.kind.editable_wire().is_none(),
                distance,
            ))
        })
        .min_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| left.2.total_cmp(&right.2))
        })
        .map(|(edge, _, _)| edge)
}

fn wire_secondary_click_hit(
    edges: &[RenderedEdge],
    position: egui::Pos2,
) -> Option<WireSecondaryClickHit> {
    rendered_edge_at_position(edges, position).map(|edge| {
        edge.kind.editable_wire().map_or(
            WireSecondaryClickHit::DisplayOnly,
            WireSecondaryClickHit::Editable,
        )
    })
}

fn editable_wire_sort_key(target: NodeEditorEditableWire) -> (u8, u8, Uuid, Uuid) {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => {
            (0, 0, connection_id, Uuid::nil())
        }
        NodeEditorEditableWire::OutputBinding { owner, node_id } => {
            let owner_kind = match owner {
                PortOwner::Composition(_) => 0,
                PortOwner::Track(_) => 1,
                PortOwner::Clip(_) => 2,
                PortOwner::Node(_) => 3,
            };
            (1, owner_kind, owner.id(), node_id)
        }
    }
}

fn editable_wire_qa_value(target: NodeEditorEditableWire) -> serde_json::Value {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => serde_json::json!({
            "kind": "explicit",
            "connection_id": connection_id,
        }),
        NodeEditorEditableWire::OutputBinding { owner, node_id } => serde_json::json!({
            "kind": "output_binding",
            "owner": qa_container_key(owner),
            "node_id": node_id,
        }),
    }
}

fn editable_wire_stable_key(target: NodeEditorEditableWire) -> String {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => connection_id.to_string(),
        NodeEditorEditableWire::OutputBinding { owner, node_id } => {
            format!("output_binding:{}:{node_id}", qa_container_key(owner))
        }
    }
}

fn editable_wire_is_current(project: &Project, target: NodeEditorEditableWire) -> bool {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => project
            .connections
            .iter()
            .any(|connection| connection.id == connection_id),
        NodeEditorEditableWire::OutputBinding { owner, node_id } => {
            container_output_node_id(project, owner) == Some(node_id)
        }
    }
}

fn rendered_wire_drag_kind(edge: &RenderedEdge, position: egui::Pos2) -> NodeEditorWireDragKind {
    // At overview zoom a whole wire can be shorter than two fixed endpoint
    // radii. Reserve at most the outer quarter on either side so the rendered
    // curve midpoint always remains a genuine body/disconnect target.
    let endpoint_radius = WIRE_ENDPOINT_RADIUS.min(edge.start.distance(edge.end) * 0.25);
    if position.distance(edge.start) <= endpoint_radius {
        NodeEditorWireDragKind::ReconnectSource
    } else if position.distance(edge.end) <= endpoint_radius {
        NodeEditorWireDragKind::ReconnectTarget
    } else {
        NodeEditorWireDragKind::Disconnect
    }
}

fn rendered_port_at_position(
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    direction: PortDirection,
    position: egui::Pos2,
    canvas_clip: egui::Rect,
) -> Option<PortAddress> {
    ports
        .iter()
        .filter(|(key, rect)| {
            key.direction == direction
                && canvas_clip.contains(position)
                && wire_port_drop_rect(**rect).contains(position)
        })
        .min_by(|left, right| {
            left.1
                .center()
                .distance(position)
                .total_cmp(&right.1.center().distance(position))
        })
        .map(|(key, _)| key.address.clone())
}

fn rendered_normal_port_at_position(
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    position: egui::Pos2,
    canvas_clip: egui::Rect,
) -> Option<RenderedPortKey> {
    canvas_clip.contains(position).then_some(())?;
    ports
        .iter()
        .filter(|(_, rect)| rect.is_positive() && rect.contains(position))
        .min_by(|left, right| {
            left.1
                .center()
                .distance(position)
                .total_cmp(&right.1.center().distance(position))
        })
        .map(|(key, _)| key.clone())
}

struct WireInteractionFrame<'a> {
    project: &'a Project,
    edges: &'a [RenderedEdge],
    rendered_ports: &'a Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    canvas_clip: egui::Rect,
    graph_item_rects: &'a [egui::Rect],
    to_global: egui::emath::TSTransform,
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

fn wire_knife_interaction(
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

fn wire_interactions(
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

fn overview_wire_graph_points(
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

fn container_resize_interactions(
    ui: &mut egui::Ui,
    project: &Project,
    containers: &[ContainerVisual],
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    state: &mut NodeEditorState,
) -> Vec<LayoutEdit> {
    let resize_interactions = node_editor_resize_interactions_enabled(to_global.scaling);
    let pointer = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.pointer.interact_pos(),
        )
    });

    let mut hit = None;
    for container in containers.iter().rev() {
        if container.collapsed {
            continue;
        }
        let global = to_global * container.rect();
        for (edge, label, unclipped_rect, cursor) in resize_regions(global) {
            let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
            let id = format!(
                "node_editor.resize_edge.{}:{label}",
                qa_container_key(container.owner)
            );
            #[cfg(test)]
            capture_test_rect(&id, rect);
            crate::qa::register_component_with_metadata(
                id.clone(),
                "node_container_resize_edge",
                rect,
                resize_interactions,
                Some(serde_json::json!({
                    "owner": qa_container_key(container.owner),
                    "edge": label,
                    "visual": false,
                    "overview_disabled": !resize_interactions,
                    "unclipped_rect": qa_rect_metadata(unclipped_rect),
                    "visible_in_canvas": rect.is_positive(),
                })),
            );
            if resize_interactions && rect.is_positive() {
                ui.interact(rect, egui::Id::new(id), egui::Sense::hover())
                    .on_hover_cursor(cursor);
            }
            if resize_interactions
                && rect.is_positive()
                && hit.is_none()
                && pointer.3.is_some_and(|position| rect.contains(position))
            {
                hit = Some((container, edge));
            }
        }
    }

    if !resize_interactions {
        state.container_resize = None;
        return Vec::new();
    }

    if state.container_resize.is_none() && pointer.0 {
        if let (Some((container, edge)), Some(position)) = (hit, pointer.3) {
            state.container_resize = Some(ContainerResizeState {
                owner: container.owner,
                edge,
                start_pointer: position,
                start_position: container.position,
                start_size: container.size,
            });
        }
    }

    let mut edits = Vec::new();
    if pointer.1 {
        if let (Some(resize), Some(position)) = (&state.container_resize, pointer.3) {
            let from_global = to_global.inverse();
            let start = from_global * resize.start_pointer;
            let current = from_global * position;
            let delta = current - start;
            let child_bounds = container_child_bounds(project, resize.owner);
            let (position, size) = resized_container_geometry(resize, delta, child_bounds);
            edits.push(LayoutEdit::ResizeContainer {
                owner: resize.owner,
                position,
                size,
            });
        }
    }
    if pointer.2 {
        state.container_resize = None;
    }
    edits
}

fn resize_regions(
    rect: egui::Rect,
) -> [(
    ContainerResizeEdge,
    &'static str,
    egui::Rect,
    egui::CursorIcon,
); 8] {
    let corner = RESIZE_CORNER_SIZE
        .min(rect.width() * 0.25)
        .min(rect.height() * 0.25);
    let h = RESIZE_HIT_WIDTH;
    [
        (
            ContainerResizeEdge::TopLeft,
            "tl",
            egui::Rect::from_min_size(rect.min, egui::vec2(corner, corner)),
            egui::CursorIcon::ResizeNwSe,
        ),
        (
            ContainerResizeEdge::TopRight,
            "tr",
            egui::Rect::from_min_size(
                egui::pos2(rect.right() - corner, rect.top()),
                egui::vec2(corner, corner),
            ),
            egui::CursorIcon::ResizeNeSw,
        ),
        (
            ContainerResizeEdge::BottomLeft,
            "bl",
            egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - corner),
                egui::vec2(corner, corner),
            ),
            egui::CursorIcon::ResizeNeSw,
        ),
        (
            ContainerResizeEdge::BottomRight,
            "br",
            egui::Rect::from_min_size(
                egui::pos2(rect.right() - corner, rect.bottom() - corner),
                egui::vec2(corner, corner),
            ),
            egui::CursorIcon::ResizeNwSe,
        ),
        (
            ContainerResizeEdge::Top,
            "top",
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + corner, rect.top() - h * 0.5),
                egui::pos2(rect.right() - corner, rect.top() + h * 0.5),
            ),
            egui::CursorIcon::ResizeVertical,
        ),
        (
            ContainerResizeEdge::Bottom,
            "bottom",
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + corner, rect.bottom() - h * 0.5),
                egui::pos2(rect.right() - corner, rect.bottom() + h * 0.5),
            ),
            egui::CursorIcon::ResizeVertical,
        ),
        (
            ContainerResizeEdge::Left,
            "left",
            egui::Rect::from_min_max(
                egui::pos2(rect.left() - h * 0.5, rect.top() + corner),
                egui::pos2(rect.left() + h * 0.5, rect.bottom() - corner),
            ),
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            ContainerResizeEdge::Right,
            "right",
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - h * 0.5, rect.top() + corner),
                egui::pos2(rect.right() + h * 0.5, rect.bottom() - corner),
            ),
            egui::CursorIcon::ResizeHorizontal,
        ),
    ]
}

fn resized_container_geometry(
    resize: &ContainerResizeState,
    delta: egui::Vec2,
    child_bounds: Option<egui::Rect>,
) -> ([f32; 2], [f32; 2]) {
    let mut left = resize.start_position[0];
    let mut top = resize.start_position[1];
    let mut right = left + resize.start_size[0];
    let mut bottom = top + resize.start_size[1];
    let moves_left = matches!(
        resize.edge,
        ContainerResizeEdge::Left | ContainerResizeEdge::TopLeft | ContainerResizeEdge::BottomLeft
    );
    let moves_right = matches!(
        resize.edge,
        ContainerResizeEdge::Right
            | ContainerResizeEdge::TopRight
            | ContainerResizeEdge::BottomRight
    );
    let moves_top = matches!(
        resize.edge,
        ContainerResizeEdge::Top | ContainerResizeEdge::TopLeft | ContainerResizeEdge::TopRight
    );
    let moves_bottom = matches!(
        resize.edge,
        ContainerResizeEdge::Bottom
            | ContainerResizeEdge::BottomLeft
            | ContainerResizeEdge::BottomRight
    );
    if moves_left {
        left += delta.x;
    }
    if moves_right {
        right += delta.x;
    }
    if moves_top {
        top += delta.y;
    }
    if moves_bottom {
        bottom += delta.y;
    }
    if let Some(children) = child_bounds {
        left = left.min(children.left() - AUTO_LAYOUT_NODE_PADDING);
        top = top.min(children.top() - CONTAINER_HEADER_HEIGHT - AUTO_LAYOUT_NODE_PADDING);
        right = right.max(children.right() + AUTO_LAYOUT_NODE_PADDING);
        bottom = bottom.max(children.bottom() + AUTO_LAYOUT_NODE_PADDING);
    }
    if right - left < MIN_CONTAINER_SIZE.x {
        if moves_left && !moves_right {
            left = right - MIN_CONTAINER_SIZE.x;
        } else {
            right = left + MIN_CONTAINER_SIZE.x;
        }
    }
    if bottom - top < MIN_CONTAINER_SIZE.y {
        if moves_top && !moves_bottom {
            top = bottom - MIN_CONTAINER_SIZE.y;
        } else {
            bottom = top + MIN_CONTAINER_SIZE.y;
        }
    }
    ([left, top], [right - left, bottom - top])
}

fn container_child_bounds(project: &Project, owner: PortOwner) -> Option<egui::Rect> {
    let mut rect = egui::Rect::NOTHING;
    let mut include = |child: egui::Rect| rect = rect.union(child);
    match owner {
        PortOwner::Composition(id) => {
            let item = project.get_composition(id)?;
            for track_id in &item.track_ids {
                if let Some(track) = project.get_track(*track_id) {
                    include(container_rect(track.ui_position, track.ui_size));
                }
            }
            for node_id in &item.node_ids {
                if let Some(node) = estimated_node_rect(project, *node_id) {
                    include(node);
                }
            }
        }
        PortOwner::Track(id) => {
            let item = project.get_track(id)?;
            for clip_id in &item.clip_ids {
                if let Some(clip) = project.get_clip(*clip_id) {
                    include(container_rect(clip.ui_position, clip.ui_size));
                }
            }
            for node_id in &item.node_ids {
                if let Some(node) = estimated_node_rect(project, *node_id) {
                    include(node);
                }
            }
        }
        PortOwner::Clip(id) => {
            let item = project.get_clip(id)?;
            for node_id in &item.node_ids {
                if let Some(node) = estimated_node_rect(project, *node_id) {
                    include(node);
                }
            }
        }
        PortOwner::Node(_) => return None,
    }
    rect.is_positive().then_some(rect)
}

fn node_selection_after_snarl_click(
    current_targets: &[SelectionTarget],
    current_primary: Option<SelectionTarget>,
    snarl_selected_node_ids: &[Uuid],
    clicked_node_id: Uuid,
    modifiers: egui::Modifiers,
) -> (Vec<SelectionTarget>, Option<SelectionTarget>) {
    // egui-snarl applies Shift/Cmd selection changes before `Snarl::show`
    // returns. Cmd without Shift is its deselect gesture. In that case the
    // clicked Node is intentionally absent from the post-show snapshot; do
    // not mistake that absence for an update race and select it again.
    if modifiers.shift {
        let mut targets = current_targets
            .iter()
            .copied()
            .filter(|target| !matches!(target, SelectionTarget::Node(_)))
            .collect::<Vec<_>>();
        for node_id in snarl_selected_node_ids {
            let target = SelectionTarget::Node(*node_id);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        let clicked = SelectionTarget::Node(clicked_node_id);
        if !targets.contains(&clicked) {
            targets.push(clicked);
        }
        return (targets, Some(clicked));
    }

    if modifiers.command {
        let clicked = SelectionTarget::Node(clicked_node_id);
        let targets = current_targets
            .iter()
            .copied()
            .filter(|target| *target != clicked)
            .collect::<Vec<_>>();
        let primary = current_primary
            .filter(|target| targets.contains(target))
            .or_else(|| targets.last().copied());
        return (targets, primary);
    }

    if snarl_selected_node_ids.contains(&clicked_node_id) {
        let targets = snarl_selected_node_ids
            .iter()
            .copied()
            .map(SelectionTarget::Node)
            .collect::<Vec<_>>();
        return (targets, Some(SelectionTarget::Node(clicked_node_id)));
    }

    let target = SelectionTarget::Node(clicked_node_id);
    (vec![target], Some(target))
}

pub fn node_editor_panel(
    ui: &mut egui::Ui,
    project_lock: &Arc<RwLock<Project>>,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
) {
    let comp_id = editor_context.active_composition_id;
    let current_time = f64::from(editor_context.timeline.current_time);
    let context_menu_state = &mut editor_context.node_editor_context_menu;
    let node_editor_state = &mut editor_context.node_editor_state;
    let Some(comp_id) = comp_id else {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
        ui.centered_and_justified(|ui| ui.label("No Composition Selected"));
        return;
    };
    let pending_owner_left_composition = node_editor_state
        .pending_continuous_edit
        .as_ref()
        .is_some_and(|pending| {
            project_lock.read().map_or(true, |project| {
                port_owner_composition(&project, pending.owner) != Some(comp_id)
            })
        });
    if pending_owner_left_composition {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    let mut requested_layout = None;
    let mut selected_nodes = editor_context
        .selection
        .targets()
        .iter()
        .filter_map(|target| target.node_id())
        .collect::<Vec<_>>();
    selected_nodes.sort_unstable();
    let selected_container = editor_context
        .selection
        .primary()
        .and_then(|target| match target {
            SelectionTarget::Composition(id) => Some(PortOwner::Composition(id)),
            SelectionTarget::Track(id) => Some(PortOwner::Track(id)),
            SelectionTarget::Clip(id) => Some(PortOwner::Clip(id)),
            SelectionTarget::Node(id) => project_lock
                .read()
                .ok()
                .and_then(|project| project.find_node_container(id))
                .map(port_owner_for_node_container),
        })
        .unwrap_or(PortOwner::Composition(comp_id));
    ui.horizontal(|ui| {
        non_selectable_label(ui, "Clean layout");
        if ui
            .button("All")
            .on_hover_text("Lay out every track and node in this composition")
            .clicked()
        {
            requested_layout = Some(AutoLayoutScope::All);
        }
        if ui
            .add_enabled(!selected_nodes.is_empty(), egui::Button::new("Selection"))
            .on_hover_text("Lay out selected nodes without moving unselected nodes")
            .clicked()
        {
            requested_layout = Some(AutoLayoutScope::Selection(selected_nodes.clone()));
        }
        if ui
            .button("Container")
            .on_hover_text("Lay out the selected track, or the composition if no track is selected")
            .clicked()
        {
            requested_layout = Some(AutoLayoutScope::Container(selected_container));
        }
    });
    ui.separator();

    if requested_layout.is_some() {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    let canvas_rect = ui.available_rect_before_wrap();
    crate::qa::register_component_with_metadata(
        "node_editor.canvas",
        "node_editor_canvas",
        canvas_rect,
        true,
        Some(serde_json::json!({"composition_id": comp_id})),
    );

    // Old projects and newly assembled container trees can carry positions
    // that were chosen before the final Snarl node dimensions were known.
    // Repair only invalid layouts (overlap or containment violations); valid
    // user-authored layouts remain untouched.
    let automatic_layout_changed = if node_editor_state.repaired_compositions.insert(comp_id) {
        let plan = project_lock.read().ok().and_then(|project| {
            layout_needs_reflow(&project, comp_id)
                .then(|| compute_full_composition_layout(&project, comp_id))
                .flatten()
        });
        plan.is_some_and(|plan| {
            project_lock
                .write()
                .is_ok_and(|mut project| apply_auto_layout(&mut project, comp_id, &plan))
        })
    } else {
        false
    };

    let explicit_layout_changed = requested_layout.is_some_and(|scope| {
        let Ok(mut project) = project_lock.write() else {
            return false;
        };
        let Some(plan) = compute_auto_layout(&project, comp_id, scope) else {
            return false;
        };
        apply_auto_layout(&mut project, comp_id, &plan)
    });

    let mut snarl;
    let layout_edits;
    let rendered_edges;
    let mut suppress_wire_secondary_click = false;
    let mut edits = Vec::new();
    let mut drop_intents = Vec::new();
    let mut pending_selection = None;
    let mut context_menu_exclusion_rects = Vec::new();
    let mut wire_context_request = None;
    let mut snarl_selected_node_ids: Vec<Uuid>;
    let mut to_global = egui::emath::TSTransform::default();
    let mut canvas_clip = canvas_rect;
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
    let plugin_manager = project_service.get_plugin_manager();
    {
        let Ok(project) = project_lock.read() else {
            ui.label("Project is unavailable");
            return;
        };
        let (built_snarl, containers) = build_snarl(&project, comp_id);
        snarl = built_snarl;

        let mut viewer = ProjectNodeViewer {
            project: &project,
            plugin_manager: Some(plugin_manager.as_ref()),
            containers: &containers,
            edits: &mut edits,
            pending_navigation: &mut node_editor_state.pending_navigation,
            pending_selection: &mut pending_selection,
            current_time,
            context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
            wire_context_request: &mut wire_context_request,
            suppress_wire_connect: node_editor_state.wire_gesture.is_some()
                || node_editor_state.normal_connect_gesture.is_some()
                || node_editor_state.normal_connect_cancel_pending_release,
            locked_canvas_transform: node_editor_state
                .normal_connect_gesture
                .as_ref()
                .map(|gesture| gesture.canvas_transform)
                .or_else(|| {
                    node_editor_state
                        .wire_knife
                        .as_ref()
                        .map(|gesture| gesture.canvas_transform)
                }),
            to_global: &mut to_global,
            canvas_clip: &mut canvas_clip,
            rendered_ports: Arc::clone(&rendered_ports),
            rendered_node_rects: Arc::clone(&rendered_node_rects),
        };
        let snarl_style = node_editor_snarl_style();
        let graph_id = egui::Id::new(("project_node_editor", comp_id));
        snarl.show(&mut viewer, &snarl_style, graph_id, ui);
        drop(viewer);
        // The early registration makes the canvas discoverable even if graph
        // construction exits before Snarl renders. Replace it in the same
        // frame with the transform observed by Snarl's `final_transform`
        // callback so HTTP QA never reads the identity placeholder.
        crate::qa::register_component_with_metadata(
            "node_editor.canvas",
            "node_editor_canvas",
            canvas_rect,
            true,
            Some(node_editor_canvas_metadata(comp_id, to_global)),
        );

        // `Snarl::show` hashes the supplied salt with the parent Ui id. Using
        // the raw salt here creates an unrelated, untransformed layer and
        // leaks graph-space chrome over the rest of the application.
        let snarl_id = ui.make_persistent_id(graph_id);
        snarl_selected_node_ids = egui_snarl::ui::get_selected_nodes(snarl_id, ui.ctx())
            .into_iter()
            .filter_map(|snarl_node_id| match snarl.get_node(snarl_node_id) {
                Some(GraphItem::Node(node_id)) => Some(*node_id),
                Some(GraphItem::Container(_) | GraphItem::PortAnchor { .. }) | None => None,
            })
            .collect();
        snarl_selected_node_ids.sort_unstable();
        let captured_drag_node_id = captured_snarl_drag_node(ui.ctx(), &snarl, snarl_id);
        let graph_layer = egui::LayerId::new(ui.layer_id().order, snarl_id);
        // `Context::layer_painter` starts with a *global* content clip. Calling
        // `with_clip_rect` with this graph-space rect would intersect two
        // coordinate systems and reduce the overview to a tiny sliver after
        // the layer transform. Construct the painter with the graph clip
        // directly; egui transforms both the shapes and this clip exactly once.
        let foreground = egui::Painter::new(
            ui.ctx().clone(),
            graph_layer,
            to_global.inverse() * canvas_clip,
        );
        let overview =
            (!node_editor_details_visible(to_global.scaling)).then_some(OverviewWirePainter {
                painter: &foreground,
                to_global,
            });
        rendered_edges = register_rendered_edges(&project, &rendered_ports, canvas_clip, overview);
        for container in &containers {
            paint_container_foreground(
                &foreground,
                &project,
                container,
                container_inactive(&project, container.owner, current_time),
                to_global.scaling,
            );
            register_container_chrome(container, to_global, canvas_clip, &project, current_time);
        }

        if ui.input(|input| input.pointer.secondary_clicked()) {
            node_editor_state.selected_connection_id = None;
            if let Some(position) = ui.input(|input| input.pointer.interact_pos()) {
                let graph_position = to_global.inverse() * position;
                let over_graph_item = context_menu_exclusion_rects
                    .iter()
                    .any(|rect| rect.contains(graph_position));
                if over_graph_item {
                    wire_context_request = None;
                } else if let Some(hit) = wire_secondary_click_hit(&rendered_edges, position) {
                    // Every rendered wire owns its secondary-click hit area.
                    // Authored wires open commands below; containment-derived
                    // wires are display-only and must not leak the same click
                    // through to the blank-canvas Add menu.
                    suppress_wire_secondary_click = true;
                    match hit {
                        WireSecondaryClickHit::Editable(target) => {
                            if wire_context_request.is_none() {
                                wire_context_request = Some(target);
                            }
                        }
                        WireSecondaryClickHit::DisplayOnly => {
                            wire_context_request = None;
                            node_editor_state.wire_context_menu = None;
                            *context_menu_state = None;
                        }
                    }
                }
            }
        }
        if let Some(target) = wire_context_request {
            let (position, open_time) = ui.input(|input| {
                (
                    input.pointer.interact_pos().unwrap_or(canvas_clip.center()),
                    input.time,
                )
            });
            node_editor_state.selected_connection_id = match target {
                NodeEditorEditableWire::ProjectConnection { connection_id } => Some(connection_id),
                NodeEditorEditableWire::OutputBinding { .. } => None,
            };
            node_editor_state.wire_context_menu = Some(NodeEditorWireContextMenu {
                target,
                position,
                open_time,
                inserting: false,
            });
            *context_menu_state = None;
        }
        edits.extend(wire_interactions(
            ui,
            node_editor_state,
            WireInteractionFrame {
                project: &project,
                edges: &rendered_edges,
                rendered_ports: &rendered_ports,
                canvas_clip,
                graph_item_rects: &context_menu_exclusion_rects,
                to_global,
            },
        ));
        if let Some(edit) = show_wire_context_menu(
            ui,
            node_editor_state,
            &project,
            plugin_manager.as_ref(),
            comp_id,
            to_global,
        ) {
            edits.push(edit);
        }

        let mut collected = collect_layout_edits(&project, &snarl);
        collected.extend(container_resize_interactions(
            ui,
            &project,
            &containers,
            to_global,
            canvas_clip,
            node_editor_state,
        ));
        let (primary_down, primary_released, pointer_position) = ui.input(|input| {
            (
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.pointer.interact_pos(),
            )
        });
        let gesture_allowed = (primary_down || primary_released)
            && node_editor_state.container_resize.is_none()
            && node_editor_state.wire_gesture.is_none()
            && node_editor_state.normal_connect_gesture.is_none()
            && node_editor_state.wire_knife.is_none();
        record_node_reparent_origins(&project, &collected, node_editor_state, gesture_allowed);
        if let (Some(pointer_position), Ok(node_rects)) =
            (pointer_position, rendered_node_rects.lock())
        {
            let graph_drop_point = to_global.inverse() * pointer_position;
            if let Some(state) = node_editor_state.node_reparent.as_mut() {
                if state.primary_node_id.is_none() {
                    state.primary_node_id =
                        captured_drag_node_id.filter(|node_id| state.origins.contains_key(node_id));
                }
            }
            if let Some(gesture) = node_editor_state.node_reparent.as_ref().cloned() {
                let final_positions = final_node_positions(&project, &gesture, &collected);
                drop_intents = node_drop_intents(
                    &project,
                    comp_id,
                    &gesture,
                    &node_rects,
                    &final_positions,
                    graph_drop_point,
                    to_global.scaling,
                );
                let active = primary_node_drop_intent(&drop_intents, graph_drop_point);
                if let Some(state) = node_editor_state.node_reparent.as_mut() {
                    state.hovered_target = active.map(|intent| intent.target.container);
                    state.hovered_node_id = active.map(|intent| intent.node_id);
                    state.hovered_score = active.map(|intent| intent.target.score);
                }
                if let Some(active) = active {
                    register_reparent_drop_targets(
                        &project,
                        comp_id,
                        active,
                        graph_drop_point,
                        to_global,
                        canvas_clip,
                        &foreground,
                    );
                }
            }
        }
        if !primary_down && !primary_released {
            node_editor_state.node_reparent = None;
            node_editor_state.moved_node_ids.clear();
        }
        layout_edits = collected;
    }

    let selection_modifiers = ui.input(|input| input.modifiers);
    let mut selection_changed = false;
    if let Some(owner) = pending_selection {
        if let Ok(project) = project_lock.read() {
            match owner {
                PortOwner::Node(node_id) if project.get_node(node_id).is_some() => {
                    let (targets, primary) = node_selection_after_snarl_click(
                        editor_context.selection.targets(),
                        editor_context.selection.primary(),
                        &snarl_selected_node_ids,
                        node_id,
                        selection_modifiers,
                    );
                    editor_context.selection.replace(targets, primary);
                    selection_changed = true;
                }
                PortOwner::Clip(clip_id) if project.get_clip(clip_id).is_some() => {
                    editor_context.selection.replace(
                        [SelectionTarget::Clip(clip_id)],
                        Some(SelectionTarget::Clip(clip_id)),
                    );
                    selection_changed = true;
                }
                PortOwner::Track(track_id) if project.get_track(track_id).is_some() => {
                    editor_context.selection.replace(
                        [SelectionTarget::Track(track_id)],
                        Some(SelectionTarget::Track(track_id)),
                    );
                    selection_changed = true;
                }
                PortOwner::Composition(composition_id) if composition_id == comp_id => {
                    editor_context.selection.replace(
                        [SelectionTarget::Composition(composition_id)],
                        Some(SelectionTarget::Composition(composition_id)),
                    );
                    selection_changed = true;
                }
                _ => {}
            }
        }
    }

    if !selection_changed
        && ui.input(|input| input.pointer.primary_released())
        && snarl_selected_node_ids.len() > 1
    {
        let targets = snarl_selected_node_ids
            .iter()
            .copied()
            .map(SelectionTarget::Node)
            .collect::<Vec<_>>();
        let primary = editor_context
            .selection
            .primary()
            .filter(|target| targets.contains(target))
            .or_else(|| targets.first().copied());
        editor_context.selection.replace(targets, primary);
        selection_changed = true;
    }
    if selection_changed {
        editor_context.interaction.preview_selected_instance_path = None;
    }

    let primary_released = ui.input(|input| input.pointer.primary_released());

    let mut layout_changed = false;
    if let Ok(mut project) = project_lock.write() {
        apply_queued_node_edits(&mut project, edits, history_manager, node_editor_state);
        for edit in layout_edits {
            layout_changed |= apply_layout_edit(&mut project, edit);
        }
        if primary_released {
            let reparent_gesture = node_editor_state.node_reparent.take();
            let moved_node_ids = reparent_gesture
                .as_ref()
                .map(|gesture| gesture.origins.keys().copied().collect::<HashSet<_>>())
                .unwrap_or_default();
            let captured_primary_node_id = reparent_gesture
                .as_ref()
                .and_then(|gesture| gesture.primary_node_id);
            node_editor_state.moved_node_ids.clear();
            let reparent_outcome =
                finish_node_reparent(&mut project, &drop_intents, reparent_gesture.as_ref());
            layout_changed |= reparent_outcome != ReparentReleaseOutcome::NoIntent;
            let dropped_wire = ui
                .input(|input| input.pointer.interact_pos())
                .and_then(|position| rendered_edge_at_position(&rendered_edges, position))
                .and_then(|edge| edge.kind.connection_id());
            if let (Some(connection_id), Some(node_id)) = (
                dropped_wire,
                splice_node_for_release(
                    reparent_outcome,
                    &moved_node_ids,
                    captured_primary_node_id,
                ),
            ) {
                if node_can_splice_connection(&project, connection_id, node_id) {
                    layout_changed |= apply_edit(
                        &mut project,
                        NodeEdit::SpliceExistingNode {
                            connection_id,
                            node_id,
                        },
                    );
                }
            }
        }
    }
    if selection_changed {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    node_editor_state.layout_changed_during_drag |= layout_changed;
    if ui.input(|input| input.pointer.secondary_clicked()) {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }
    let created = handle_context_menu(
        ui,
        context_menu_state,
        NodeContextMenuFrame {
            project_lock,
            project_service,
            comp_id,
            exclusion_rects: &context_menu_exclusion_rects,
            to_global,
            suppress_secondary_click: suppress_wire_secondary_click
                || wire_context_request.is_some(),
        },
    );
    // Creation already places its item in a free slot and grows only the
    // necessary ancestors. Connections change dependency semantics, not
    // geometry, and deletion cannot violate containment. Reflowing the entire
    // composition for any of those edits destroys user-authored positions and
    // can move a newly created Node outside the current viewport. Full layout
    // remains available explicitly and for the one-time invalid-layout repair.
    let layout_finished = primary_released && node_editor_state.layout_changed_during_drag;
    if layout_finished {
        node_editor_state.layout_changed_during_drag = false;
    }
    if automatic_layout_changed || explicit_layout_changed || created || layout_finished {
        push_history_snapshot(project_lock, history_manager);
    }
    if let Ok(project) = project_lock.read() {
        editor_context.reconcile_selection(&project);
    }
}

fn build_snarl(project: &Project, comp_id: Uuid) -> (Snarl<GraphItem>, Vec<ContainerVisual>) {
    let mut snarl = Snarl::new();
    let mut snarl_ids = HashMap::new();
    let mut containers = Vec::new();

    let Some(composition) = project.get_composition(comp_id) else {
        return (snarl, containers);
    };
    let composition_visual = ContainerVisual {
        owner: PortOwner::Composition(composition.id),
        kind: ContainerKind::Composition,
        position: composition.ui_position,
        size: composition.ui_size,
        collapsed: composition.ui_collapsed,
    };
    insert_container_items(&composition_visual, &mut snarl, &mut snarl_ids);
    containers.push(composition_visual);

    if !composition.ui_collapsed {
        insert_leaf_nodes(project, &composition.node_ids, &mut snarl, &mut snarl_ids);
        for track_id in &composition.track_ids {
            let Some(track) = project.get_track(*track_id) else {
                continue;
            };
            let track_visual = ContainerVisual {
                owner: PortOwner::Track(track.id),
                kind: ContainerKind::Track,
                position: track.ui_position,
                size: track.ui_size,
                collapsed: track.ui_collapsed,
            };
            insert_container_items(&track_visual, &mut snarl, &mut snarl_ids);
            containers.push(track_visual);

            if !track.ui_collapsed {
                insert_leaf_nodes(project, &track.node_ids, &mut snarl, &mut snarl_ids);
                for clip_id in &track.clip_ids {
                    let Some(clip) = project.get_clip(*clip_id) else {
                        continue;
                    };
                    let clip_visual = ContainerVisual {
                        owner: PortOwner::Clip(clip.id),
                        kind: ContainerKind::Clip,
                        position: clip.ui_position,
                        size: clip.ui_size,
                        collapsed: clip.ui_collapsed,
                    };
                    insert_container_items(&clip_visual, &mut snarl, &mut snarl_ids);
                    containers.push(clip_visual);
                    if !clip.ui_collapsed {
                        insert_leaf_nodes(project, &clip.node_ids, &mut snarl, &mut snarl_ids);
                    }
                }
            }
        }
    }

    for connection in &project.connections {
        let Some(source_item) = output_graph_item(project, &connection.from) else {
            continue;
        };
        let Some(target_item) = input_graph_item(project, &connection.to) else {
            continue;
        };
        let (Some(source_snarl_id), Some(target_snarl_id)) =
            (snarl_ids.get(&source_item), snarl_ids.get(&target_item))
        else {
            continue;
        };
        let Some(output_index) = output_definitions(project, source_item)
            .iter()
            .position(|output| output.key == connection.from.port)
        else {
            continue;
        };
        let Some(input_index) = input_definitions(project, target_item)
            .iter()
            .position(|input| input.key == connection.to.port)
        else {
            continue;
        };
        snarl.connect(
            egui_snarl::OutPinId {
                node: *source_snarl_id,
                output: output_index,
            },
            egui_snarl::InPinId {
                node: *target_snarl_id,
                input: input_index,
            },
        );
    }

    connect_container_output_wires(project, &containers, &snarl_ids, &mut snarl);

    (snarl, containers)
}

fn connect_container_output_wires(
    project: &Project,
    containers: &[ContainerVisual],
    snarl_ids: &HashMap<GraphItem, egui_snarl::NodeId>,
    snarl: &mut Snarl<GraphItem>,
) {
    for visual in containers {
        let sink_item = GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ImageSink,
        };
        let Some(&sink_id) = snarl_ids.get(&sink_item) else {
            continue;
        };
        for source in project.container_image_sources(visual.owner) {
            let source_owner = source.source;
            let source_item = match source_owner {
                PortOwner::Node(id) => GraphItem::Node(id),
                owner @ (PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_)) => {
                    GraphItem::PortAnchor {
                        owner,
                        kind: PortAnchorKind::ExternalImage,
                    }
                }
            };
            let Some(&source_id) = snarl_ids.get(&source_item) else {
                continue;
            };
            let Some(output_index) =
                output_definitions(project, source_item)
                    .iter()
                    .position(|definition| {
                        definition.key == library::model::project::IMAGE_OUTPUT_PORT
                    })
            else {
                continue;
            };
            snarl.connect(
                egui_snarl::OutPinId {
                    node: source_id,
                    output: output_index,
                },
                egui_snarl::InPinId {
                    node: sink_id,
                    input: 0,
                },
            );
        }
    }
}

fn insert_leaf_nodes(
    project: &Project,
    node_ids: &[Uuid],
    snarl: &mut Snarl<GraphItem>,
    snarl_ids: &mut HashMap<GraphItem, egui_snarl::NodeId>,
) {
    for node_id in node_ids {
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        let item = GraphItem::Node(*node_id);
        if snarl_ids.contains_key(&item) {
            continue;
        }
        let position = node.ui_position;
        let snarl_id = snarl.insert_node(egui::pos2(position[0], position[1]), item);
        snarl_ids.insert(item, snarl_id);
    }
}

fn insert_container_items(
    visual: &ContainerVisual,
    snarl: &mut Snarl<GraphItem>,
    snarl_ids: &mut HashMap<GraphItem, egui_snarl::NodeId>,
) {
    let items = [
        GraphItem::Container(visual.owner),
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ExternalInputs,
        },
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::InternalMetadata,
        },
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ImageSink,
        },
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ExternalImage,
        },
    ];

    for item in items {
        let position = container_item_position(visual, item);
        let node_id = snarl.insert_node(position, item);
        snarl_ids.insert(item, node_id);
    }
}

fn container_item_position(visual: &ContainerVisual, item: GraphItem) -> egui::Pos2 {
    let position = egui::pos2(visual.position[0], visual.position[1]);
    let size = egui::vec2(
        visual.size[0].max(MIN_CONTAINER_SIZE.x),
        visual.size[1].max(MIN_CONTAINER_SIZE.y),
    );
    let port_y = if visual.collapsed {
        12.0
    } else {
        CONTAINER_PORT_Y
    };
    match item {
        GraphItem::Container(_) => position + CONTAINER_CONTROL_OFFSET,
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ExternalInputs,
            ..
        } => egui::pos2(position.x - 14.0, position.y + port_y),
        GraphItem::PortAnchor {
            kind: PortAnchorKind::InternalMetadata,
            ..
        } => egui::pos2(position.x + 2.0, position.y + port_y),
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ImageSink,
            ..
        } => egui::pos2(
            position.x + size.x - 16.0,
            position.y + CONTAINER_HEADER_HEIGHT * 0.5,
        ),
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ExternalImage,
            ..
        } => egui::pos2(
            position.x + size.x - 2.0,
            position.y + CONTAINER_HEADER_HEIGHT * 0.5,
        ),
        GraphItem::Node(_) => position,
    }
}

fn output_graph_item(project: &Project, address: &PortAddress) -> Option<GraphItem> {
    let definition = project.port_definition(address, PortDirection::Output)?;
    match address.owner {
        PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_) => {
            match definition.side {
                PortSide::Left => Some(GraphItem::PortAnchor {
                    owner: address.owner,
                    kind: PortAnchorKind::InternalMetadata,
                }),
                PortSide::Right => Some(GraphItem::PortAnchor {
                    owner: address.owner,
                    kind: PortAnchorKind::ExternalImage,
                }),
            }
        }
        PortOwner::Node(node_id) if project.get_node(node_id).is_some() => {
            Some(GraphItem::Node(node_id))
        }
        PortOwner::Node(_) => None,
    }
}

fn input_graph_item(project: &Project, address: &PortAddress) -> Option<GraphItem> {
    project.port_definition(address, PortDirection::Input)?;
    match address.owner {
        PortOwner::Composition(id) if project.get_composition(id).is_some() => {
            Some(GraphItem::PortAnchor {
                owner: address.owner,
                kind: PortAnchorKind::ExternalInputs,
            })
        }
        PortOwner::Track(id) if project.get_track(id).is_some() => Some(GraphItem::PortAnchor {
            owner: address.owner,
            kind: PortAnchorKind::ExternalInputs,
        }),
        PortOwner::Clip(id) if project.get_clip(id).is_some() => Some(GraphItem::PortAnchor {
            owner: address.owner,
            kind: PortAnchorKind::ExternalInputs,
        }),
        PortOwner::Node(id) if project.get_node(id).is_some() => Some(GraphItem::Node(id)),
        _ => None,
    }
}

fn container_visual(project: &Project, owner: PortOwner) -> Option<ContainerVisual> {
    match owner {
        PortOwner::Composition(id) => {
            project
                .get_composition(id)
                .map(|composition| ContainerVisual {
                    owner,
                    kind: ContainerKind::Composition,
                    position: composition.ui_position,
                    size: composition.ui_size,
                    collapsed: composition.ui_collapsed,
                })
        }
        PortOwner::Track(id) => project.get_track(id).map(|track| ContainerVisual {
            owner,
            kind: ContainerKind::Track,
            position: track.ui_position,
            size: track.ui_size,
            collapsed: track.ui_collapsed,
        }),
        PortOwner::Clip(id) => project.get_clip(id).map(|clip| ContainerVisual {
            owner,
            kind: ContainerKind::Clip,
            position: clip.ui_position,
            size: clip.ui_size,
            collapsed: clip.ui_collapsed,
        }),
        PortOwner::Node(_) => None,
    }
}

fn collect_layout_edits(project: &Project, snarl: &Snarl<GraphItem>) -> Vec<LayoutEdit> {
    let mut edits = Vec::new();
    for (position, item) in snarl.nodes_pos() {
        match *item {
            GraphItem::Node(node_id) => {
                let Some(node) = project.get_node(node_id) else {
                    continue;
                };
                let new_position = [position.x, position.y];
                if node.ui_position != new_position {
                    edits.push(LayoutEdit::MoveNode {
                        node_id,
                        position: new_position,
                    });
                }
            }
            GraphItem::Container(owner) => {
                let Some(visual) = container_visual(project, owner) else {
                    continue;
                };
                let expected = container_item_position(&visual, *item);
                let delta = position - expected;
                if delta.length_sq() > 0.001 {
                    edits.push(LayoutEdit::MoveContainer {
                        owner,
                        delta: [delta.x, delta.y],
                    });
                }
            }
            GraphItem::PortAnchor { .. } => {}
        }
    }
    edits
}

fn layout_needs_reflow(project: &Project, composition_id: Uuid) -> bool {
    let Some(composition) = project.get_composition(composition_id) else {
        return false;
    };
    let composition_rect = container_rect(composition.ui_position, composition.ui_size);
    let composition_content = composition_content_rect(composition_rect);
    let mut node_rects = Vec::<(Uuid, egui::Rect)>::new();

    for node_id in &composition.node_ids {
        let Some(rect) = estimated_node_rect(project, *node_id) else {
            continue;
        };
        if !rect_contains_rect(composition_content, rect) {
            return true;
        }
        node_rects.push((*node_id, rect));
    }

    let mut track_rects = Vec::new();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        if !rect_contains_rect(composition_content, track_rect) {
            return true;
        }
        if track_rects
            .iter()
            .any(|other: &egui::Rect| padded_intersection(*other, track_rect))
        {
            return true;
        }
        if composition.node_ids.iter().any(|node_id| {
            estimated_node_rect(project, *node_id)
                .is_some_and(|rect| padded_intersection(rect, track_rect))
        }) {
            return true;
        }
        track_rects.push(track_rect);

        let track_content = nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP);
        let mut clip_rects = Vec::new();
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            if !rect_contains_rect(track_content, clip_rect)
                || clip_rects
                    .iter()
                    .any(|other: &egui::Rect| padded_intersection(*other, clip_rect))
            {
                return true;
            }
            if track.node_ids.iter().any(|node_id| {
                estimated_node_rect(project, *node_id)
                    .is_some_and(|rect| padded_intersection(rect, clip_rect))
            }) {
                return true;
            }
            clip_rects.push(clip_rect);

            let clip_content = nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP);
            for node_id in &clip.node_ids {
                let Some(rect) = estimated_node_rect(project, *node_id) else {
                    continue;
                };
                if !rect_contains_rect(clip_content, rect) {
                    return true;
                }
                node_rects.push((*node_id, rect));
            }
        }
        for node_id in &track.node_ids {
            let Some(rect) = estimated_node_rect(project, *node_id) else {
                continue;
            };
            if !rect_contains_rect(track_content, rect) {
                return true;
            }
            node_rects.push((*node_id, rect));
        }
    }

    if node_rects.iter().enumerate().any(|(index, (_, left))| {
        node_rects[index + 1..]
            .iter()
            .any(|(_, right)| padded_intersection(*left, *right))
    }) {
        return true;
    }

    let all_nodes = node_rects.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let ranks = rank_nodes_by_scc(&all_nodes, &canonical_edges(project, &all_nodes));
    let rects = node_rects.into_iter().collect::<HashMap<_, _>>();
    canonical_edges(project, &all_nodes)
        .into_iter()
        .any(|(from, to)| {
            ranks.get(&from) != ranks.get(&to)
                && rects
                    .get(&from)
                    .zip(rects.get(&to))
                    .is_some_and(|(from_rect, to_rect)| {
                        from_rect.right() + AUTO_LAYOUT_NODE_PADDING > to_rect.left()
                    })
        })
}

fn container_hierarchy_needs_reflow(project: &Project, composition_id: Uuid) -> bool {
    let Some(composition) = project.get_composition(composition_id) else {
        return false;
    };
    let composition_content =
        composition_content_rect(container_rect(composition.ui_position, composition.ui_size));
    if composition.node_ids.iter().any(|node_id| {
        estimated_node_rect(project, *node_id)
            .is_some_and(|rect| !rect_contains_rect(composition_content, rect))
    }) {
        return true;
    }
    let mut track_rects = Vec::new();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        if !rect_contains_rect(composition_content, track_rect)
            || track_rects
                .iter()
                .any(|other| padded_intersection(*other, track_rect))
            || composition.node_ids.iter().any(|node_id| {
                estimated_node_rect(project, *node_id)
                    .is_some_and(|rect| padded_intersection(rect, track_rect))
            })
        {
            return true;
        }
        track_rects.push(track_rect);
        let track_content = nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP);
        if track.node_ids.iter().any(|node_id| {
            estimated_node_rect(project, *node_id)
                .is_some_and(|rect| !rect_contains_rect(track_content, rect))
        }) {
            return true;
        }
        let mut clip_rects = Vec::new();
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            if !rect_contains_rect(track_content, clip_rect)
                || clip_rects
                    .iter()
                    .any(|other| padded_intersection(*other, clip_rect))
                || track.node_ids.iter().any(|node_id| {
                    estimated_node_rect(project, *node_id)
                        .is_some_and(|rect| padded_intersection(rect, clip_rect))
                })
            {
                return true;
            }
            let clip_content = nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP);
            if clip.node_ids.iter().any(|node_id| {
                estimated_node_rect(project, *node_id)
                    .is_some_and(|rect| !rect_contains_rect(clip_content, rect))
            }) {
                return true;
            }
            clip_rects.push(clip_rect);
        }
    }
    false
}

fn estimated_node_rect(project: &Project, node_id: Uuid) -> Option<egui::Rect> {
    let node = project.get_node(node_id)?;
    Some(egui::Rect::from_min_size(
        egui::pos2(node.ui_position[0], node.ui_position[1]),
        estimated_node_size(project, node_id),
    ))
}

fn composition_content_rect(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(AUTO_LAYOUT_COMPOSITION_LEFT, AUTO_LAYOUT_COMPOSITION_TOP),
        rect.max
            - egui::vec2(
                AUTO_LAYOUT_COMPOSITION_RIGHT,
                AUTO_LAYOUT_COMPOSITION_BOTTOM,
            ),
    )
}

fn nested_content_rect(rect: egui::Rect, top: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(AUTO_LAYOUT_TRACK_LEFT, top),
        rect.max - egui::vec2(AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_BOTTOM),
    )
}

fn rect_contains_rect(outer: egui::Rect, inner: egui::Rect) -> bool {
    outer.min.x <= inner.min.x + f32::EPSILON
        && outer.min.y <= inner.min.y + f32::EPSILON
        && outer.max.x + f32::EPSILON >= inner.max.x
        && outer.max.y + f32::EPSILON >= inner.max.y
}

fn padded_intersection(left: egui::Rect, right: egui::Rect) -> bool {
    left.expand(AUTO_LAYOUT_NODE_PADDING * 0.5)
        .intersects(right.expand(AUTO_LAYOUT_NODE_PADDING * 0.5))
}

fn apply_layout_edit(project: &mut Project, edit: LayoutEdit) -> bool {
    match edit {
        LayoutEdit::MoveNode { node_id, position } => {
            let Some(node) = project.get_node_mut(node_id) else {
                return false;
            };
            if node.ui_position == position {
                return false;
            }
            node.ui_position = position;
            true
        }
        LayoutEdit::MoveContainer { owner, delta } => translate_container(project, owner, delta),
        LayoutEdit::ResizeContainer {
            owner,
            position,
            size,
        } => set_container_geometry(project, owner, position, size),
    }
}

fn compute_auto_layout(
    project: &Project,
    composition_id: Uuid,
    scope: AutoLayoutScope,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    match scope {
        AutoLayoutScope::All => compute_full_composition_layout(project, composition_id),
        AutoLayoutScope::Container(PortOwner::Composition(id)) if id == composition_id => {
            compute_full_composition_layout(project, composition_id)
        }
        AutoLayoutScope::Container(PortOwner::Track(id)) if composition.track_ids.contains(&id) => {
            compute_track_scope_layout(project, composition_id, id)
        }
        AutoLayoutScope::Container(PortOwner::Clip(id))
            if composition.track_ids.iter().any(|track_id| {
                project
                    .get_track(*track_id)
                    .is_some_and(|track| track.clip_ids.contains(&id))
            }) =>
        {
            compute_clip_scope_layout(project, composition_id, id)
        }
        AutoLayoutScope::Selection(selected) => {
            compute_selection_scope_layout(project, composition_id, &selected)
        }
        AutoLayoutScope::Container(_) => None,
    }
}

fn composition_graph_node_ids(project: &Project, composition_id: Uuid) -> Vec<Uuid> {
    let Some(composition) = project.get_composition(composition_id) else {
        return Vec::new();
    };
    let mut node_ids = composition.node_ids.clone();
    for track_id in &composition.track_ids {
        node_ids.extend(track_leaf_node_ids(project, *track_id));
    }
    node_ids.sort_unstable();
    node_ids.dedup();
    node_ids
}

fn compute_track_scope_layout(
    project: &Project,
    composition_id: Uuid,
    track_id: Uuid,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let track = project.get_track(track_id)?;
    let full = compute_full_composition_layout(project, composition_id)?;
    let full_track = *full.track_layouts.get(&track_id)?;
    let occupied = composition
        .track_ids
        .iter()
        .filter(|candidate| **candidate != track_id)
        .filter_map(|candidate| project.get_track(*candidate))
        .map(|candidate| container_rect(candidate.ui_position, candidate.ui_size))
        .collect::<Vec<_>>();
    let y = first_free_y(
        full_track.position[0],
        full_track.size[0],
        full_track.size[1],
        track.ui_position[1].max(composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP),
        &occupied,
        AUTO_LAYOUT_TRACK_GAP,
    );
    let delta_y = y - full_track.position[1];
    let mut plan = AutoLayoutPlan::default();
    plan.track_layouts.insert(
        track_id,
        ContainerLayout {
            position: [full_track.position[0], y],
            size: full_track.size,
        },
    );
    for clip_id in &track.clip_ids {
        if let Some(layout) = full.clip_layouts.get(clip_id) {
            plan.clip_layouts.insert(
                *clip_id,
                ContainerLayout {
                    position: [layout.position[0], layout.position[1] + delta_y],
                    size: layout.size,
                },
            );
        }
    }
    for node_id in track_leaf_node_ids(project, track_id) {
        if let Some(position) = full.node_positions.get(&node_id) {
            plan.node_positions
                .insert(node_id, [position[0], position[1] + delta_y]);
        }
    }
    let target_rect = egui::Rect::from_min_size(
        egui::pos2(full_track.position[0], y),
        egui::vec2(full_track.size[0], full_track.size[1]),
    );
    plan.composition_size = Some(grown_size_for_rect(
        composition.ui_position,
        composition.ui_size,
        target_rect,
        AUTO_LAYOUT_COMPOSITION_RIGHT,
        AUTO_LAYOUT_COMPOSITION_BOTTOM,
    ));
    Some(plan)
}

fn compute_clip_scope_layout(
    project: &Project,
    composition_id: Uuid,
    clip_id: Uuid,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let track_id = project.find_track_for_clip(clip_id)?;
    let track = project.get_track(track_id)?;
    let clip = project.get_clip(clip_id)?;
    let full = compute_full_composition_layout(project, composition_id)?;
    let full_clip = *full.clip_layouts.get(&clip_id)?;
    let occupied = track
        .clip_ids
        .iter()
        .filter(|candidate| **candidate != clip_id)
        .filter_map(|candidate| project.get_clip(*candidate))
        .map(|candidate| container_rect(candidate.ui_position, candidate.ui_size))
        .chain(
            track
                .node_ids
                .iter()
                .filter_map(|node_id| estimated_node_rect(project, *node_id)),
        )
        .collect::<Vec<_>>();
    let x = full_clip.position[0].max(track.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT);
    let y = first_free_y(
        x,
        full_clip.size[0],
        full_clip.size[1],
        clip.ui_position[1].max(track.ui_position[1] + AUTO_LAYOUT_TRACK_TOP),
        &occupied,
        AUTO_LAYOUT_ROW_GAP,
    );
    let delta = [x - full_clip.position[0], y - full_clip.position[1]];
    let mut plan = AutoLayoutPlan::default();
    plan.clip_layouts.insert(
        clip_id,
        ContainerLayout {
            position: [x, y],
            size: full_clip.size,
        },
    );
    for node_id in &clip.node_ids {
        if let Some(position) = full.node_positions.get(node_id) {
            plan.node_positions
                .insert(*node_id, [position[0] + delta[0], position[1] + delta[1]]);
        }
    }
    let clip_rect = egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(full_clip.size[0], full_clip.size[1]),
    );
    let track_size = grown_size_for_rect(
        track.ui_position,
        track.ui_size,
        clip_rect,
        AUTO_LAYOUT_TRACK_RIGHT,
        AUTO_LAYOUT_TRACK_BOTTOM,
    );
    plan.track_layouts.insert(
        track_id,
        ContainerLayout {
            position: track.ui_position,
            size: track_size,
        },
    );
    let track_rect = egui::Rect::from_min_size(
        egui::pos2(track.ui_position[0], track.ui_position[1]),
        egui::vec2(track_size[0], track_size[1]),
    );
    plan.composition_size = Some(grown_size_for_rect(
        composition.ui_position,
        composition.ui_size,
        track_rect,
        AUTO_LAYOUT_COMPOSITION_RIGHT,
        AUTO_LAYOUT_COMPOSITION_BOTTOM,
    ));
    Some(plan)
}

fn compute_selection_scope_layout(
    project: &Project,
    composition_id: Uuid,
    selected: &[Uuid],
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let graph_nodes = composition_graph_node_ids(project, composition_id);
    let graph_node_set = graph_nodes.iter().copied().collect::<HashSet<_>>();
    let mut selected = selected
        .iter()
        .copied()
        .filter(|node_id| graph_node_set.contains(node_id))
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    if selected.is_empty() {
        return None;
    }

    let full = compute_full_composition_layout(project, composition_id)?;
    selected.sort_by(|left, right| {
        full.node_positions[left][0]
            .total_cmp(&full.node_positions[right][0])
            .then_with(|| left.cmp(right))
    });
    let selected_set = selected.iter().copied().collect::<HashSet<_>>();
    let mut occupied = graph_nodes
        .iter()
        .filter(|node_id| !selected_set.contains(node_id))
        .filter_map(|node_id| estimated_node_rect(project, *node_id))
        .collect::<Vec<_>>();
    let mut plan = AutoLayoutPlan {
        composition_size: Some(composition.ui_size),
        ..AutoLayoutPlan::default()
    };

    for node_id in selected {
        let container = project.find_node_container(node_id)?;
        let desired = *full.node_positions.get(&node_id)?;
        let size = estimated_node_size(project, node_id);
        let (owner_position, _) = planned_container_geometry(project, &plan, container)?;
        let left_inset = match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_LEFT,
            NodeContainer::Track(_) | NodeContainer::Clip(_) => AUTO_LAYOUT_TRACK_LEFT,
        };
        let top_inset = match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
            NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
            NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
        };
        let x = desired[0].max(owner_position[0] + left_inset);
        let child_obstacles = immediate_child_rects(project, &plan, container);
        let mut local_occupied = occupied.clone();
        local_occupied.extend(child_obstacles);
        let y = first_free_y(
            x,
            size.x,
            size.y,
            owner_position[1] + top_inset,
            &local_occupied,
            AUTO_LAYOUT_ROW_GAP,
        );
        let node_rect = egui::Rect::from_min_size(egui::pos2(x, y), size);
        plan.node_positions.insert(node_id, [x, y]);
        occupied.push(node_rect);
        grow_planned_hierarchy(project, composition_id, container, node_rect, &mut plan);
    }
    Some(plan)
}

fn planned_container_geometry(
    project: &Project,
    plan: &AutoLayoutPlan,
    container: NodeContainer,
) -> Option<([f32; 2], [f32; 2])> {
    match container {
        NodeContainer::Composition(id) => project.get_composition(id).map(|item| {
            (
                item.ui_position,
                plan.composition_size.unwrap_or(item.ui_size),
            )
        }),
        NodeContainer::Track(id) => project.get_track(id).map(|item| {
            plan.track_layouts
                .get(&id)
                .map_or((item.ui_position, item.ui_size), |layout| {
                    (layout.position, layout.size)
                })
        }),
        NodeContainer::Clip(id) => project.get_clip(id).map(|item| {
            plan.clip_layouts
                .get(&id)
                .map_or((item.ui_position, item.ui_size), |layout| {
                    (layout.position, layout.size)
                })
        }),
    }
}

fn immediate_child_rects(
    project: &Project,
    plan: &AutoLayoutPlan,
    container: NodeContainer,
) -> Vec<egui::Rect> {
    match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .into_iter()
            .flat_map(|composition| composition.track_ids.iter())
            .filter_map(|track_id| {
                planned_container_geometry(project, plan, NodeContainer::Track(*track_id))
            })
            .map(|(position, size)| container_rect(position, size))
            .collect(),
        NodeContainer::Track(id) => project
            .get_track(id)
            .into_iter()
            .flat_map(|track| track.clip_ids.iter())
            .filter_map(|clip_id| {
                planned_container_geometry(project, plan, NodeContainer::Clip(*clip_id))
            })
            .map(|(position, size)| container_rect(position, size))
            .collect(),
        NodeContainer::Clip(_) => Vec::new(),
    }
}

fn grow_planned_hierarchy(
    project: &Project,
    composition_id: Uuid,
    container: NodeContainer,
    rect: egui::Rect,
    plan: &mut AutoLayoutPlan,
) {
    let mut ancestor_rect = rect;
    if let NodeContainer::Clip(clip_id) = container {
        if let Some(clip) = project.get_clip(clip_id) {
            let current = plan
                .clip_layouts
                .get(&clip_id)
                .copied()
                .unwrap_or(ContainerLayout {
                    position: clip.ui_position,
                    size: clip.ui_size,
                });
            let size = grown_size_for_rect(
                current.position,
                current.size,
                rect,
                AUTO_LAYOUT_TRACK_RIGHT,
                AUTO_LAYOUT_TRACK_BOTTOM,
            );
            plan.clip_layouts.insert(
                clip_id,
                ContainerLayout {
                    position: current.position,
                    size,
                },
            );
            ancestor_rect = container_rect(current.position, size);
        }
    }

    let track_id = match container {
        NodeContainer::Track(id) => Some(id),
        NodeContainer::Clip(id) => project.find_track_for_clip(id),
        NodeContainer::Composition(_) => None,
    };
    if let Some(track_id) = track_id {
        if let Some(track) = project.get_track(track_id) {
            let current = plan
                .track_layouts
                .get(&track_id)
                .copied()
                .unwrap_or(ContainerLayout {
                    position: track.ui_position,
                    size: track.ui_size,
                });
            let size = grown_size_for_rect(
                current.position,
                current.size,
                ancestor_rect,
                AUTO_LAYOUT_TRACK_RIGHT,
                AUTO_LAYOUT_TRACK_BOTTOM,
            );
            plan.track_layouts.insert(
                track_id,
                ContainerLayout {
                    position: current.position,
                    size,
                },
            );
            ancestor_rect = container_rect(current.position, size);
        }
    }

    if let Some(composition) = project.get_composition(composition_id) {
        plan.composition_size = Some(grown_size_for_rect(
            composition.ui_position,
            plan.composition_size.unwrap_or(composition.ui_size),
            ancestor_rect,
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ));
    }
}

fn grown_size_for_rect(
    position: [f32; 2],
    current_size: [f32; 2],
    rect: egui::Rect,
    right_margin: f32,
    bottom_margin: f32,
) -> [f32; 2] {
    [
        current_size[0].max(rect.right() - position[0] + right_margin),
        current_size[1].max(rect.bottom() - position[1] + bottom_margin),
    ]
}

fn track_leaf_node_ids(project: &Project, track_id: Uuid) -> Vec<Uuid> {
    let Some(track) = project.get_track(track_id) else {
        return Vec::new();
    };
    let mut node_ids = track.node_ids.clone();
    for clip_id in &track.clip_ids {
        if let Some(clip) = project.get_clip(*clip_id) {
            node_ids.extend(clip.node_ids.iter().copied());
        }
    }
    node_ids
}

fn compute_full_composition_layout(
    project: &Project,
    composition_id: Uuid,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let mut nodes = composition.node_ids.clone();
    for track_id in &composition.track_ids {
        nodes.extend(track_leaf_node_ids(project, *track_id));
    }
    nodes.sort_unstable();
    nodes.dedup();
    let edges = canonical_edges(project, &nodes);
    let ranks = rank_nodes_by_scc(&nodes, &edges);
    let mut plan = AutoLayoutPlan::default();
    let track_x = composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_LEFT;
    // Every leaf Node shares this composition-space rank axis. Container
    // membership may move a Node vertically, but can never reverse an edge by
    // giving two sibling Clips or Tracks unrelated local x origins.
    let column_origin_x = track_x + AUTO_LAYOUT_TRACK_LEFT * 2.0;
    let rank_columns = node_rank_columns(project, &nodes, &ranks, column_origin_x);
    let mut track_y = composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP;
    let mut composition_right = track_x;
    let mut composition_bottom = track_y;

    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let content_y = track_y + AUTO_LAYOUT_TRACK_TOP;
        let default_clip_x = track_x + AUTO_LAYOUT_TRACK_LEFT;
        let mut required_right = default_clip_x;
        let mut required_bottom = content_y;
        let mut occupied = Vec::<egui::Rect>::new();

        let mut clip_ids = track.clip_ids.clone();
        clip_ids.sort_by_key(|clip_id| {
            project
                .get_clip(*clip_id)
                .map_or((usize::MAX, *clip_id), |clip| {
                    let minimum_rank = clip
                        .node_ids
                        .iter()
                        .filter_map(|node_id| ranks.get(node_id))
                        .copied()
                        .min()
                        .unwrap_or_default();
                    (minimum_rank, *clip_id)
                })
        });
        for clip_id in clip_ids {
            let Some(clip) = project.get_clip(clip_id) else {
                continue;
            };
            let band = node_band_bounds(project, &clip.node_ids, &ranks, &rank_columns);
            let clip_x = band.map_or(default_clip_x, |bounds| {
                bounds.min_x - AUTO_LAYOUT_TRACK_LEFT
            });
            let clip_width = band.map_or(MIN_CONTAINER_SIZE.x, |bounds| {
                (bounds.max_x + AUTO_LAYOUT_TRACK_RIGHT - clip_x).max(MIN_CONTAINER_SIZE.x)
            });
            let clip_height = band.map_or(MIN_CONTAINER_SIZE.y, |bounds| {
                (AUTO_LAYOUT_CLIP_TOP + bounds.height + AUTO_LAYOUT_TRACK_BOTTOM)
                    .max(MIN_CONTAINER_SIZE.y)
            });
            let clip_y = first_free_y(
                clip_x,
                clip_width,
                clip_height,
                content_y,
                &occupied,
                AUTO_LAYOUT_CLIP_GAP,
            );
            let clip_rect = egui::Rect::from_min_size(
                egui::pos2(clip_x, clip_y),
                egui::vec2(clip_width, clip_height),
            );
            let _ = layout_node_band(
                project,
                &clip.node_ids,
                &ranks,
                &rank_columns,
                clip_y + AUTO_LAYOUT_CLIP_TOP,
                &mut plan.node_positions,
            );
            plan.clip_layouts.insert(
                clip_id,
                ContainerLayout {
                    position: [clip_x, clip_y],
                    size: [clip_width, clip_height],
                },
            );
            required_right = required_right.max(clip_rect.right());
            required_bottom = required_bottom.max(clip_rect.bottom());
            occupied.push(clip_rect);
        }

        if let Some(bounds) = node_band_bounds(project, &track.node_ids, &ranks, &rank_columns) {
            let direct_y = first_free_y(
                bounds.min_x,
                bounds.width(),
                bounds.height,
                content_y,
                &occupied,
                AUTO_LAYOUT_ROW_GAP,
            );
            let _ = layout_node_band(
                project,
                &track.node_ids,
                &ranks,
                &rank_columns,
                direct_y,
                &mut plan.node_positions,
            );
            required_right = required_right.max(bounds.max_x);
            required_bottom = required_bottom.max(direct_y + bounds.height);
        }

        let track_size = [
            (required_right - track_x + AUTO_LAYOUT_TRACK_RIGHT).max(MIN_CONTAINER_SIZE.x),
            (required_bottom - track_y + AUTO_LAYOUT_TRACK_BOTTOM).max(MIN_CONTAINER_SIZE.y),
        ];
        plan.track_layouts.insert(
            *track_id,
            ContainerLayout {
                position: [track_x, track_y],
                size: track_size,
            },
        );
        composition_right = composition_right.max(track_x + track_size[0]);
        composition_bottom = composition_bottom.max(track_y + track_size[1]);
        track_y += track_size[1] + AUTO_LAYOUT_TRACK_GAP;
    }

    if let Some(bounds) = node_band_bounds(project, &composition.node_ids, &ranks, &rank_columns) {
        let direct_y = if composition.track_ids.is_empty() {
            composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP
        } else {
            composition_bottom + AUTO_LAYOUT_TRACK_GAP
        };
        let _ = layout_node_band(
            project,
            &composition.node_ids,
            &ranks,
            &rank_columns,
            direct_y,
            &mut plan.node_positions,
        );
        composition_right = composition_right.max(bounds.max_x);
        composition_bottom = composition_bottom.max(direct_y + bounds.height);
    }

    plan.composition_size = Some([
        (composition_right - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT)
            .max(MIN_CONTAINER_SIZE.x),
        (composition_bottom - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM)
            .max(MIN_CONTAINER_SIZE.y),
    ]);
    Some(plan)
}

#[derive(Clone, Copy, Debug)]
struct NodeBandBounds {
    min_x: f32,
    max_x: f32,
    height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NodeRankColumn {
    x: f32,
    width: f32,
}

fn node_rank_columns(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    column_origin_x: f32,
) -> BTreeMap<usize, NodeRankColumn> {
    let mut widths = BTreeMap::<usize, f32>::new();
    for node_id in node_ids {
        if project.get_node(*node_id).is_none() {
            continue;
        }
        let rank = ranks.get(node_id).copied().unwrap_or_default();
        let width = estimated_node_size(project, *node_id).x;
        widths
            .entry(rank)
            .and_modify(|column_width| *column_width = column_width.max(width))
            .or_insert(width);
    }
    let Some(max_rank) = widths.keys().next_back().copied() else {
        return BTreeMap::new();
    };
    let mut columns = BTreeMap::new();
    let mut x = column_origin_x;
    for rank in 0..=max_rank {
        let width = widths
            .get(&rank)
            .copied()
            .unwrap_or_else(estimated_node_width);
        columns.insert(rank, NodeRankColumn { x, width });
        x += width + AUTO_LAYOUT_COLUMN_GAP;
    }
    columns
}

impl NodeBandBounds {
    fn width(self) -> f32 {
        self.max_x - self.min_x
    }
}

fn node_band_bounds(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    rank_columns: &BTreeMap<usize, NodeRankColumn>,
) -> Option<NodeBandBounds> {
    let mut column_heights = BTreeMap::<usize, f32>::new();
    for node_id in node_ids {
        if project.get_node(*node_id).is_none() {
            continue;
        }
        let rank = ranks.get(node_id).copied().unwrap_or_default();
        let column_height = column_heights.entry(rank).or_default();
        if *column_height > 0.0 {
            *column_height += AUTO_LAYOUT_ROW_GAP;
        }
        *column_height += estimated_node_size(project, *node_id).y;
    }
    let min_rank = column_heights.keys().next().copied()?;
    let max_rank = column_heights.keys().next_back().copied()?;
    let min_column = rank_columns.get(&min_rank)?;
    let max_column = rank_columns.get(&max_rank)?;
    Some(NodeBandBounds {
        min_x: min_column.x,
        max_x: max_column.x + max_column.width,
        height: column_heights
            .values()
            .copied()
            .max_by(f32::total_cmp)
            .unwrap_or_default(),
    })
}

fn layout_node_band(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    rank_columns: &BTreeMap<usize, NodeRankColumn>,
    origin_y: f32,
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
) -> Option<NodeBandBounds> {
    let mut groups = BTreeMap::<usize, Vec<Uuid>>::new();
    for node_id in node_ids {
        if project.get_node(*node_id).is_some() {
            groups
                .entry(ranks.get(node_id).copied().unwrap_or_default())
                .or_default()
                .push(*node_id);
        }
    }
    for group in groups.values_mut() {
        group.sort_by(|left, right| {
            let left_y = project
                .get_node(*left)
                .map_or(0.0, |node| node.ui_position[1]);
            let right_y = project
                .get_node(*right)
                .map_or(0.0, |node| node.ui_position[1]);
            left_y.total_cmp(&right_y).then_with(|| left.cmp(right))
        });
    }

    let bounds = node_band_bounds(project, node_ids, ranks, rank_columns)?;
    for (rank, group) in groups {
        let x = rank_columns.get(&rank)?.x;
        let mut y = origin_y;
        for node_id in group {
            let size = estimated_node_size(project, node_id);
            positions.insert(node_id, [x, y]);
            y += size.y + AUTO_LAYOUT_ROW_GAP;
        }
    }
    Some(bounds)
}

fn first_free_y(
    x: f32,
    width: f32,
    height: f32,
    initial_y: f32,
    occupied: &[egui::Rect],
    gap: f32,
) -> f32 {
    let mut y = initial_y;
    loop {
        let candidate = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height));
        let next_y = occupied
            .iter()
            .filter(|other| rects_are_closer_than(candidate, **other, gap))
            .map(|other| other.bottom() + gap)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            return y;
        };
        y = next_y;
    }
}

fn rects_are_closer_than(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
    left.left() < right.right() + gap
        && left.right() + gap > right.left()
        && left.top() < right.bottom() + gap
        && left.bottom() + gap > right.top()
}

fn canonical_edges(project: &Project, nodes: &[Uuid]) -> Vec<(Uuid, Uuid)> {
    let node_set = nodes.iter().copied().collect::<HashSet<_>>();
    let mut edges = project
        .connections
        .iter()
        .filter_map(|connection| {
            let (PortOwner::Node(from), PortOwner::Node(to)) =
                (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            (node_set.contains(&from)
                && node_set.contains(&to)
                && project.get_node(from).is_some()
                && project.get_node(to).is_some())
            .then_some((from, to))
        })
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    edges
}

fn rank_nodes_by_scc(nodes: &[Uuid], edges: &[(Uuid, Uuid)]) -> HashMap<Uuid, usize> {
    let node_set = nodes.iter().copied().collect::<HashSet<_>>();
    let mut adjacency = HashMap::<Uuid, Vec<Uuid>>::new();
    for node_id in nodes {
        adjacency.entry(*node_id).or_default();
    }
    for (from, to) in edges {
        if node_set.contains(from) && node_set.contains(to) {
            adjacency.entry(*from).or_default().push(*to);
        }
    }
    for successors in adjacency.values_mut() {
        successors.sort_unstable();
        successors.dedup();
    }

    let mut sorted_nodes = nodes.to_vec();
    sorted_nodes.sort_unstable();
    sorted_nodes.dedup();
    let mut next_index = 0;
    let mut indices = HashMap::<Uuid, usize>::new();
    let mut lowlinks = HashMap::<Uuid, usize>::new();
    let mut stack = Vec::new();
    let mut on_stack = HashSet::new();
    let mut components = Vec::<Vec<Uuid>>::new();
    for node_id in sorted_nodes {
        if !indices.contains_key(&node_id) {
            visit_scc(
                node_id,
                &adjacency,
                &mut next_index,
                &mut indices,
                &mut lowlinks,
                &mut stack,
                &mut on_stack,
                &mut components,
            );
        }
    }
    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by_key(|component| component.first().copied());

    let mut component_by_node = HashMap::new();
    for (component_index, component) in components.iter().enumerate() {
        for node_id in component {
            component_by_node.insert(*node_id, component_index);
        }
    }
    let mut outgoing = vec![BTreeSet::<usize>::new(); components.len()];
    let mut indegree = vec![0_usize; components.len()];
    for (from, to) in edges {
        let (Some(&from_component), Some(&to_component)) =
            (component_by_node.get(from), component_by_node.get(to))
        else {
            continue;
        };
        if from_component != to_component && outgoing[from_component].insert(to_component) {
            indegree[to_component] += 1;
        }
    }

    let mut ready = BTreeSet::<(Uuid, usize)>::new();
    for (index, component) in components.iter().enumerate() {
        if indegree[index] == 0 {
            if let Some(first) = component.first() {
                ready.insert((*first, index));
            }
        }
    }
    let mut component_rank = vec![0_usize; components.len()];
    while let Some(entry) = ready.iter().next().copied() {
        ready.remove(&entry);
        let component = entry.1;
        for successor in outgoing[component].iter().copied() {
            component_rank[successor] =
                component_rank[successor].max(component_rank[component] + 1);
            indegree[successor] -= 1;
            if indegree[successor] == 0 {
                ready.insert((components[successor][0], successor));
            }
        }
    }

    component_by_node
        .into_iter()
        .map(|(node_id, component)| (node_id, component_rank[component]))
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "Tarjan traversal state is intentionally explicit and stack-local"
)]
fn visit_scc(
    node_id: Uuid,
    adjacency: &HashMap<Uuid, Vec<Uuid>>,
    next_index: &mut usize,
    indices: &mut HashMap<Uuid, usize>,
    lowlinks: &mut HashMap<Uuid, usize>,
    stack: &mut Vec<Uuid>,
    on_stack: &mut HashSet<Uuid>,
    components: &mut Vec<Vec<Uuid>>,
) {
    let index = *next_index;
    *next_index += 1;
    indices.insert(node_id, index);
    lowlinks.insert(node_id, index);
    stack.push(node_id);
    on_stack.insert(node_id);

    for successor in adjacency.get(&node_id).into_iter().flatten().copied() {
        if !indices.contains_key(&successor) {
            visit_scc(
                successor, adjacency, next_index, indices, lowlinks, stack, on_stack, components,
            );
            let successor_lowlink = lowlinks[&successor];
            lowlinks
                .entry(node_id)
                .and_modify(|lowlink| *lowlink = (*lowlink).min(successor_lowlink));
        } else if on_stack.contains(&successor) {
            let successor_index = indices[&successor];
            lowlinks
                .entry(node_id)
                .and_modify(|lowlink| *lowlink = (*lowlink).min(successor_index));
        }
    }

    if lowlinks[&node_id] == indices[&node_id] {
        let mut component = Vec::new();
        while let Some(member) = stack.pop() {
            on_stack.remove(&member);
            component.push(member);
            if member == node_id {
                break;
            }
        }
        components.push(component);
    }
}

fn estimated_node_width() -> f32 {
    (NODE_BODY_WIDTH + PORT_LABEL_WIDTH * 2.0 + 70.0).max(NODE_HEADER_WIDTH + 30.0)
}

fn estimated_merge_node_width() -> f32 {
    (MERGE_BODY_WIDTH + PORT_LABEL_WIDTH * 2.0 + 84.0).max(NODE_HEADER_WIDTH + 30.0)
}

fn estimated_node_size(project: &Project, node_id: Uuid) -> egui::Vec2 {
    let item = GraphItem::Node(node_id);
    let pin_rows = input_definitions(project, item)
        .len()
        .max(output_definitions(project, item).len());
    // These are conservative graph-space bounds for the complete rendered
    // card (header, pin rows and body controls), not just the body widget.
    // The extra pin term keeps plugin Nodes with unusually many ports safe.
    let content = project.get_node(node_id).map(Node::content);
    let base_height = match content {
        Some(NodeContent::Generator(GeneratorContent::Text)) => 330.0,
        Some(NodeContent::Generator(GeneratorContent::Shape))
        | Some(NodeContent::Generator(GeneratorContent::SkSL)) => 300.0,
        Some(NodeContent::Generator(GeneratorContent::Solid)) => 240.0,
        Some(NodeContent::PluginOperation(_)) => 260.0,
        Some(NodeContent::Merge) => {
            let layer_count = merge_layer_rows(project, node_id).len();
            (166.0 + layer_count as f32 * 82.0).max(220.0)
        }
        Some(NodeContent::Media(_) | NodeContent::Reference(_) | NodeContent::Value(_)) => 220.0,
        None => 220.0,
    };
    egui::vec2(
        if matches!(content, Some(NodeContent::Merge)) {
            estimated_merge_node_width()
        } else {
            estimated_node_width()
        },
        base_height + pin_rows.saturating_sub(4) as f32 * PORT_ROW_HEIGHT,
    )
}

fn apply_auto_layout(project: &mut Project, composition_id: Uuid, plan: &AutoLayoutPlan) -> bool {
    let mut changed = false;
    for (node_id, position) in &plan.node_positions {
        let Some(node) = project.get_node_mut(*node_id) else {
            continue;
        };
        if node.ui_position != *position {
            node.ui_position = *position;
            changed = true;
        }
    }
    for (clip_id, layout) in &plan.clip_layouts {
        let Some(clip) = project.get_clip_mut(*clip_id) else {
            continue;
        };
        if clip.ui_position != layout.position {
            clip.ui_position = layout.position;
            changed = true;
        }
        if clip.ui_size != layout.size {
            clip.ui_size = layout.size;
            changed = true;
        }
    }
    for (track_id, layout) in &plan.track_layouts {
        let Some(track) = project.get_track_mut(*track_id) else {
            continue;
        };
        if track.ui_position != layout.position {
            track.ui_position = layout.position;
            changed = true;
        }
        if track.ui_size != layout.size {
            track.ui_size = layout.size;
            changed = true;
        }
    }
    if let (Some(size), Some(composition)) = (
        plan.composition_size,
        project.get_composition_mut(composition_id),
    ) {
        if composition.ui_size != size {
            composition.ui_size = size;
            changed = true;
        }
    }
    changed
}

fn translate_container(project: &mut Project, owner: PortOwner, delta: [f32; 2]) -> bool {
    if delta[0].abs() < f32::EPSILON && delta[1].abs() < f32::EPSILON {
        return false;
    }
    match owner {
        PortOwner::Composition(composition_id) => {
            let Some((track_ids, node_ids)) = project
                .get_composition(composition_id)
                .map(|composition| (composition.track_ids.clone(), composition.node_ids.clone()))
            else {
                return false;
            };
            if let Some(composition) = project.get_composition_mut(composition_id) {
                translate_position(&mut composition.ui_position, delta);
            }
            for track_id in track_ids {
                translate_track_tree(project, track_id, delta);
            }
            translate_nodes(project, &node_ids, delta);
            true
        }
        PortOwner::Track(track_id) => translate_track_tree(project, track_id, delta),
        PortOwner::Clip(clip_id) => translate_clip_tree(project, clip_id, delta),
        PortOwner::Node(_) => false,
    }
}

fn translate_track_tree(project: &mut Project, track_id: Uuid, delta: [f32; 2]) -> bool {
    let Some((clip_ids, node_ids)) = project
        .get_track(track_id)
        .map(|track| (track.clip_ids.clone(), track.node_ids.clone()))
    else {
        return false;
    };
    if let Some(track) = project.get_track_mut(track_id) {
        translate_position(&mut track.ui_position, delta);
    }
    translate_nodes(project, &node_ids, delta);
    for clip_id in clip_ids {
        translate_clip_tree(project, clip_id, delta);
    }
    true
}

fn translate_clip_tree(project: &mut Project, clip_id: Uuid, delta: [f32; 2]) -> bool {
    let Some(node_ids) = project.get_clip(clip_id).map(|clip| clip.node_ids.clone()) else {
        return false;
    };
    if let Some(clip) = project.get_clip_mut(clip_id) {
        translate_position(&mut clip.ui_position, delta);
    }
    translate_nodes(project, &node_ids, delta);
    true
}

fn translate_nodes(project: &mut Project, node_ids: &[Uuid], delta: [f32; 2]) {
    for node_id in node_ids {
        if let Some(node) = project.get_node_mut(*node_id) {
            translate_position(&mut node.ui_position, delta);
        }
    }
}

fn translate_position(position: &mut [f32; 2], delta: [f32; 2]) {
    position[0] += delta[0];
    position[1] += delta[1];
}

fn set_container_size(project: &mut Project, owner: PortOwner, size: [f32; 2]) -> bool {
    let size = [
        size[0].max(MIN_CONTAINER_SIZE.x),
        size[1].max(MIN_CONTAINER_SIZE.y),
    ];
    match owner {
        PortOwner::Composition(id) => {
            let Some(composition) = project.get_composition_mut(id) else {
                return false;
            };
            if composition.ui_size == size {
                return false;
            }
            composition.ui_size = size;
            true
        }
        PortOwner::Track(id) => {
            let Some(track) = project.get_track_mut(id) else {
                return false;
            };
            if track.ui_size == size {
                return false;
            }
            track.ui_size = size;
            true
        }
        PortOwner::Clip(id) => {
            let Some(clip) = project.get_clip_mut(id) else {
                return false;
            };
            if clip.ui_size == size {
                return false;
            }
            clip.ui_size = size;
            true
        }
        PortOwner::Node(_) => false,
    }
}

fn set_container_geometry(
    project: &mut Project,
    owner: PortOwner,
    position: [f32; 2],
    size: [f32; 2],
) -> bool {
    let size = [
        size[0].max(MIN_CONTAINER_SIZE.x),
        size[1].max(MIN_CONTAINER_SIZE.y),
    ];
    match owner {
        PortOwner::Composition(id) => project.get_composition_mut(id).is_some_and(|item| {
            if item.ui_position == position && item.ui_size == size {
                return false;
            }
            item.ui_position = position;
            item.ui_size = size;
            true
        }),
        PortOwner::Track(id) => project.get_track_mut(id).is_some_and(|item| {
            if item.ui_position == position && item.ui_size == size {
                return false;
            }
            item.ui_position = position;
            item.ui_size = size;
            true
        }),
        PortOwner::Clip(id) => project.get_clip_mut(id).is_some_and(|item| {
            if item.ui_position == position && item.ui_size == size {
                return false;
            }
            item.ui_position = position;
            item.ui_size = size;
            true
        }),
        PortOwner::Node(_) => false,
    }
}

fn node_can_splice_connection(project: &Project, connection_id: Uuid, node_id: Uuid) -> bool {
    splice_ports_for_node(project, connection_id, node_id).is_some()
}

fn splice_ports_for_node(
    project: &Project,
    connection_id: Uuid,
    node_id: Uuid,
) -> Option<(PortAddress, PortAddress)> {
    let node = project.get_node(node_id)?;
    if !matches!(
        node.content(),
        NodeContent::PluginOperation(_) | NodeContent::Merge
    ) {
        return None;
    }
    let connection = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)?;
    if [connection.from.owner, connection.to.owner].contains(&PortOwner::Node(node_id)) {
        return None;
    }
    let source = project.port_definition(&connection.from, PortDirection::Output)?;
    let target = project.port_definition(&connection.to, PortDirection::Input)?;
    let definitions = project.port_definitions(PortOwner::Node(node_id));

    let mut inputs = definitions
        .iter()
        .filter(|definition| {
            if definition.direction != PortDirection::Input
                || !definition.data_type.accepts(source.data_type)
            {
                return false;
            }
            let address = PortAddress::new(PortOwner::Node(node_id), definition.key.clone());
            definition.multiplicity == PortMultiplicity::Variadic
                || !project
                    .connections
                    .iter()
                    .any(|connection| connection.to == address)
        })
        .collect::<Vec<_>>();
    inputs.sort_by_key(|definition| {
        (
            definition.data_type != source.data_type,
            !matches!(definition.key.as_str(), "image" | "shape" | "input"),
            definition.key.clone(),
        )
    });

    let mut outputs = definitions
        .iter()
        .filter(|definition| {
            definition.direction == PortDirection::Output
                && target.data_type.accepts(definition.data_type)
        })
        .collect::<Vec<_>>();
    outputs.sort_by_key(|definition| {
        (
            definition.data_type != target.data_type,
            !matches!(definition.key.as_str(), "image" | "shape" | "output"),
            definition.key.clone(),
        )
    });

    Some((
        PortAddress::new(PortOwner::Node(node_id), inputs.first()?.key.clone()),
        PortAddress::new(PortOwner::Node(node_id), outputs.first()?.key.clone()),
    ))
}

fn splice_existing_node_on_connection(
    project: &mut Project,
    connection_id: Uuid,
    node_id: Uuid,
) -> bool {
    let Some((via_input, via_output)) = splice_ports_for_node(project, connection_id, node_id)
    else {
        return false;
    };
    match project.splice_connection(connection_id, via_input, via_output) {
        Ok(_) => true,
        Err(error) => {
            log::warn!("Cannot splice Node {node_id} into wire {connection_id}: {error}");
            false
        }
    }
}

fn insert_node_on_connection(
    project: &mut Project,
    connection_id: Uuid,
    mut node: Node,
    position: egui::Pos2,
    composition_id: Uuid,
) -> bool {
    if !project
        .connections
        .iter()
        .any(|connection| connection.id == connection_id)
    {
        return false;
    }
    let mut candidate = project.clone();
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    candidate.add_node(node);
    let Some(container) =
        attach_node_at_position(&mut candidate, node_id, composition_id, position)
    else {
        return false;
    };
    place_node_in_free_slot(&mut candidate, node_id, container, position, &[]);
    if !splice_existing_node_on_connection(&mut candidate, connection_id, node_id) {
        return false;
    }
    if let Some(rect) = estimated_node_rect(&candidate, node_id) {
        ensure_container_hierarchy_contains(&mut candidate, container, rect);
    }
    *project = candidate;
    true
}

fn container_for_output_owner(owner: PortOwner) -> Option<NodeContainer> {
    match owner {
        PortOwner::Composition(id) => Some(NodeContainer::Composition(id)),
        PortOwner::Track(id) => Some(NodeContainer::Track(id)),
        PortOwner::Clip(id) => Some(NodeContainer::Clip(id)),
        PortOwner::Node(_) => None,
    }
}

fn disconnect_editable_wires(project: &mut Project, wires: Vec<NodeEditorEditableWire>) -> bool {
    let mut wires = wires
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    wires.sort_by_key(|target| editable_wire_sort_key(*target));
    let mut candidate = project.clone();
    let mut changed = false;
    for target in wires {
        match target {
            NodeEditorEditableWire::ProjectConnection { connection_id } => {
                changed |= candidate.disconnect_connection(connection_id);
            }
            NodeEditorEditableWire::OutputBinding { owner, node_id } => {
                if container_output_node_id(&candidate, owner) != Some(node_id) {
                    continue;
                }
                let Some(container) = container_for_output_owner(owner) else {
                    return false;
                };
                if let Err(error) = candidate.set_output_node(container, None) {
                    log::warn!("Cannot clear container output binding: {error}");
                    return false;
                }
                changed = true;
            }
        }
    }
    if changed {
        *project = candidate;
    }
    changed
}

fn apply_edit(project: &mut Project, edit: NodeEdit) -> bool {
    match edit {
        NodeEdit::Connect { from, to } => match project.connect_ports(from, to) {
            Ok(_) => true,
            Err(error) => {
                log::warn!("Cannot connect project ports: {error}");
                false
            }
        },
        NodeEdit::Disconnect { from, to } => project.disconnect_ports(&from, &to),
        NodeEdit::DisconnectConnection { connection_id } => {
            project.disconnect_connection(connection_id)
        }
        NodeEdit::DisconnectWires { wires } => disconnect_editable_wires(project, wires),
        NodeEdit::ReconnectConnection {
            connection_id,
            from,
            to,
        } => match project.reconnect_connection(connection_id, from, to) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("Cannot reconnect project wire {connection_id}: {error}");
                false
            }
        },
        NodeEdit::SetConnectionBlendMode {
            connection_id,
            blend_mode,
        } => {
            let Some(connection) = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
            else {
                return false;
            };
            if connection.blend_mode == blend_mode {
                return false;
            }
            match project.set_connection_blend_mode(connection_id, blend_mode) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Cannot set authored blend on wire {connection_id}: {error}");
                    false
                }
            }
        }
        NodeEdit::ReorderConnection {
            connection_id,
            new_order,
        } => {
            let Some(connection) = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
            else {
                return false;
            };
            let mut siblings = project
                .connections
                .iter()
                .filter(|candidate| candidate.to == connection.to)
                .map(|candidate| (candidate.order, candidate.id))
                .collect::<Vec<_>>();
            siblings.sort_by_key(|(order, id)| (*order, *id));
            let Some(current_index) = siblings.iter().position(|(_, id)| *id == connection_id)
            else {
                return false;
            };
            let target_index = new_order.max(0) as usize;
            let target_index = target_index.min(siblings.len().saturating_sub(1));
            if target_index == current_index {
                return false;
            }
            match project.reorder_connection(connection_id, target_index as i64) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Cannot reorder wire {connection_id}: {error}");
                    false
                }
            }
        }
        NodeEdit::SpliceExistingNode {
            connection_id,
            node_id,
        } => splice_existing_node_on_connection(project, connection_id, node_id),
        NodeEdit::InsertNodeOnConnection {
            connection_id,
            node,
            position,
            composition_id,
        } => insert_node_on_connection(project, connection_id, *node, position, composition_id),
        NodeEdit::SetOutputNode { owner, node_id } => {
            let Some(container) = container_for_output_owner(owner) else {
                return false;
            };
            let before = container_output_node_id(project, owner);
            if before == node_id {
                return false;
            }
            match project.set_output_node(container, node_id) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Cannot set container output node: {error}");
                    false
                }
            }
        }
        NodeEdit::Delete { owner } => match owner {
            PortOwner::Node(id) => project.remove_node(id).is_some(),
            PortOwner::Clip(id) => project.remove_clip(id).is_some(),
            PortOwner::Track(id) => project.remove_track(id).is_some(),
            PortOwner::Composition(_) => false,
        },
        NodeEdit::SetEnabled { node_id, enabled } => {
            project.get_node_mut(node_id).is_some_and(|node| {
                if node.enabled == enabled {
                    return false;
                }
                node.enabled = enabled;
                true
            })
        }
        NodeEdit::RenameContainer { owner, name } => match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = project.get_composition_mut(id) else {
                    return false;
                };
                if composition.name == name {
                    return false;
                }
                composition.name = name;
                true
            }
            PortOwner::Track(id) => {
                let Some(track) = project.get_track_mut(id) else {
                    return false;
                };
                if track.name == name {
                    return false;
                }
                track.name = name;
                true
            }
            PortOwner::Clip(id) => {
                let Some(clip) = project.get_clip_mut(id) else {
                    return false;
                };
                if clip.name == name {
                    return false;
                }
                clip.name = name;
                true
            }
            PortOwner::Node(_) => false,
        },
        NodeEdit::ResizeContainer { owner, size } => set_container_size(project, owner, size),
        NodeEdit::ToggleContainer { owner } => match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = project.get_composition_mut(id) else {
                    return false;
                };
                composition.ui_collapsed = !composition.ui_collapsed;
                true
            }
            PortOwner::Track(id) => {
                let Some(track) = project.get_track_mut(id) else {
                    return false;
                };
                track.ui_collapsed = !track.ui_collapsed;
                true
            }
            PortOwner::Clip(id) => {
                let Some(clip) = project.get_clip_mut(id) else {
                    return false;
                };
                clip.ui_collapsed = !clip.ui_collapsed;
                true
            }
            PortOwner::Node(_) => false,
        },
        NodeEdit::Rename { node_id, name } => project.get_node_mut(node_id).is_some_and(|node| {
            if node.name == name {
                return false;
            }
            node.name = name;
            true
        }),
        NodeEdit::SetProperty {
            owner,
            key,
            time,
            value,
        } => match owner {
            PortOwner::Clip(id) => project
                .get_clip_mut(id)
                .is_some_and(|clip| clip.update_property_or_keyframe(&key, time, value, None)),
            PortOwner::Node(id) => project
                .get_node_mut(id)
                .is_some_and(|node| node.update_property_or_keyframe(&key, time, value, None)),
            PortOwner::Composition(_) | PortOwner::Track(_) => false,
        },
    }
}

fn flush_pending_continuous_edit_with_project(
    project: &Project,
    history_manager: &mut HistoryManager,
    node_editor_state: &mut NodeEditorState,
) -> bool {
    if node_editor_state.pending_continuous_edit.take().is_none() {
        return false;
    }
    history_manager.push_project_state(project.clone());
    true
}

fn apply_queued_node_edits(
    project: &mut Project,
    queued_edits: Vec<QueuedNodeEdit>,
    history_manager: &mut HistoryManager,
    node_editor_state: &mut NodeEditorState,
) -> bool {
    let mut changed = false;
    let mut atomic_batch_changed = false;

    for queued in queued_edits {
        match queued {
            QueuedNodeEdit::Atomic(edit) => {
                flush_pending_continuous_edit_with_project(
                    project,
                    history_manager,
                    node_editor_state,
                );
                atomic_batch_changed |= apply_edit(project, edit);
                changed |= atomic_batch_changed;
            }
            QueuedNodeEdit::Continuous {
                pending,
                edit,
                finished,
            } => {
                if atomic_batch_changed {
                    history_manager.push_project_state(project.clone());
                    atomic_batch_changed = false;
                }
                if node_editor_state
                    .pending_continuous_edit
                    .as_ref()
                    .is_some_and(|current| current != &pending)
                {
                    flush_pending_continuous_edit_with_project(
                        project,
                        history_manager,
                        node_editor_state,
                    );
                }

                if edit.is_some_and(|edit| apply_edit(project, edit)) {
                    node_editor_state.pending_continuous_edit = Some(pending.clone());
                    changed = true;
                }
                if finished && node_editor_state.pending_continuous_edit.as_ref() == Some(&pending)
                {
                    flush_pending_continuous_edit_with_project(
                        project,
                        history_manager,
                        node_editor_state,
                    );
                }
            }
        }
    }

    if atomic_batch_changed {
        history_manager.push_project_state(project.clone());
    }
    changed
}

/// Commit a dirty inline Node Editor gesture when the panel stops being the
/// active tab or its owner disappears. The Project has already been updated;
/// this records exactly one final snapshot and clears only ephemeral UI state.
pub fn flush_pending_continuous_edit(
    project_lock: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
    node_editor_state: &mut NodeEditorState,
) -> bool {
    if node_editor_state.pending_continuous_edit.is_none() {
        return false;
    }
    let Ok(project) = project_lock.read() else {
        return false;
    };
    flush_pending_continuous_edit_with_project(&project, history_manager, node_editor_state)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NodeCreateRequest {
    Text,
    Solid,
    Shape,
    SkSL,
    TimeModulo,
    Style(String),
    Effector(String),
    Decorator(String),
    Effect(String),
    Merge,
    Clip,
    Track,
    Composition,
}

impl NodeCreateRequest {
    fn qa_kind(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Solid => "solid",
            Self::Shape => "shape",
            Self::SkSL => "sksl",
            Self::TimeModulo => "time_modulo",
            Self::Style(_) => "style",
            Self::Effector(_) => "effector",
            Self::Decorator(_) => "decorator",
            Self::Effect(_) => "effect",
            Self::Merge => "merge",
            Self::Clip => "clip",
            Self::Track => "track",
            Self::Composition => "composition",
        }
    }
}

fn node_create_menu_item(
    label: impl Into<String>,
    category: impl Into<String>,
    keywords: impl IntoIterator<Item = impl Into<String>>,
    qa_id: impl Into<String>,
    value: NodeCreateRequest,
) -> SearchableItem<NodeCreateRequest> {
    let mut item = SearchableItem::new(label, value);
    item.category = Some(category.into());
    item.keywords = keywords.into_iter().map(Into::into).collect();
    item.qa_id = Some(qa_id.into());
    item.qa_metadata = Some(serde_json::json!({
        "action": "create",
        "kind": item.value.qa_kind(),
    }));
    item
}

struct PluginOperationMenuItemSpec {
    descriptor_category: &'static str,
    operation: &'static str,
    component_id: String,
    menu_category: String,
    display_kind: &'static str,
    qa_id: String,
    request: NodeCreateRequest,
    extra_keywords: Vec<String>,
}

fn plugin_operation_menu_item(
    plugin_manager: &PluginManager,
    spec: PluginOperationMenuItemSpec,
) -> Option<SearchableItem<NodeCreateRequest>> {
    let PluginOperationMenuItemSpec {
        descriptor_category,
        operation,
        component_id,
        menu_category,
        display_kind,
        qa_id,
        request,
        extra_keywords,
    } = spec;
    let descriptor = match plugin_manager.operation_descriptor(
        descriptor_category,
        &component_id,
        operation,
    ) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            log::warn!(
                "Cannot expose {descriptor_category} operation {component_id} in the Node Editor: {error}"
            );
            return None;
        }
    };
    let label = descriptor.label().to_string();
    let mut keywords = vec![
        display_kind.to_lowercase(),
        label.clone(),
        component_id.clone(),
        descriptor.category().to_string(),
        descriptor.operation().to_string(),
    ];
    keywords.extend(extra_keywords);
    let mut item = node_create_menu_item(
        format!("{display_kind} · {label}"),
        menu_category,
        keywords,
        qa_id,
        request,
    );
    item.qa_metadata = Some(serde_json::json!({
        "action": "create",
        "kind": item.value.qa_kind(),
        "component_id": component_id,
        "operation_category": descriptor.category(),
        "operation": descriptor.operation(),
        "label": descriptor.label(),
    }));
    Some(item)
}

fn node_create_menu_items(
    plugin_manager: &PluginManager,
) -> Vec<SearchableItem<NodeCreateRequest>> {
    let mut items = vec![
        node_create_menu_item(
            "Text",
            "Generators",
            ["title", "caption", "shape"],
            "node_editor.menu.create.text",
            NodeCreateRequest::Text,
        ),
        node_create_menu_item(
            "Solid Color",
            "Generators",
            ["solid", "color", "image"],
            "node_editor.menu.create.solid",
            NodeCreateRequest::Solid,
        ),
        node_create_menu_item(
            "Shape (Rectangle)",
            "Generators",
            ["shape", "rectangle", "path"],
            "node_editor.menu.create.shape",
            NodeCreateRequest::Shape,
        ),
        node_create_menu_item(
            "SkSL Shader",
            "Generators",
            ["sksl", "shader", "procedural", "image"],
            "node_editor.menu.create.sksl",
            NodeCreateRequest::SkSL,
        ),
        node_create_menu_item(
            "Time Modulo",
            "Timing / Values",
            ["time", "modulo", "loop", "remainder", "value", "number"],
            "node_editor.menu.create.time_modulo",
            NodeCreateRequest::TimeModulo,
        ),
        node_create_menu_item(
            "Merge",
            "Compositing",
            ["merge", "composite", "blend", "layers"],
            "node_editor.menu.create.merge",
            NodeCreateRequest::Merge,
        ),
        node_create_menu_item(
            "Container (Clip)",
            "Containers",
            ["clip", "container", "timeline"],
            "node_editor.menu.create.clip",
            NodeCreateRequest::Clip,
        ),
        node_create_menu_item(
            "Container (Track)",
            "Containers",
            ["track", "container", "timeline"],
            "node_editor.menu.create.track",
            NodeCreateRequest::Track,
        ),
        node_create_menu_item(
            "Container (Composition)",
            "Containers",
            ["composition", "container", "nested"],
            "node_editor.menu.create.composition",
            NodeCreateRequest::Composition,
        ),
    ];

    let mut styles = plugin_manager.get_available_styles();
    styles.sort();
    items.extend(styles.into_iter().filter_map(|component_id| {
        let qa_id = match component_id.as_str() {
            "fill" | "stroke" => format!("node_editor.menu.create.{component_id}"),
            _ => format!("node_editor.menu.create.style:{component_id}"),
        };
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: STYLE_CATEGORY,
                operation: STYLE_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Styles".to_string(),
                display_kind: "Style",
                qa_id,
                request: NodeCreateRequest::Style(component_id),
                extra_keywords: vec!["shape".to_string(), "image".to_string()],
            },
        )
    }));

    let mut effectors = plugin_manager.get_available_effectors();
    effectors.sort();
    items.extend(effectors.into_iter().filter_map(|component_id| {
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: EFFECTOR_CATEGORY,
                operation: EFFECTOR_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Effectors".to_string(),
                display_kind: "Effector",
                qa_id: format!("node_editor.menu.create.effector:{component_id}"),
                request: NodeCreateRequest::Effector(component_id),
                extra_keywords: vec!["shape".to_string()],
            },
        )
    }));

    let mut decorators = plugin_manager.get_available_decorators();
    decorators.sort();
    items.extend(decorators.into_iter().filter_map(|component_id| {
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: DECORATOR_CATEGORY,
                operation: DECORATOR_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Decorators".to_string(),
                display_kind: "Decorator",
                qa_id: format!("node_editor.menu.create.decorator:{component_id}"),
                request: NodeCreateRequest::Decorator(component_id),
                extra_keywords: vec!["shape".to_string(), "ensemble".to_string()],
            },
        )
    }));

    let mut effects = plugin_manager.get_available_effects();
    effects.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    items.extend(
        effects
            .into_iter()
            .filter_map(|(effect_id, effect_name, effect_category)| {
                plugin_operation_menu_item(
                    plugin_manager,
                    PluginOperationMenuItemSpec {
                        descriptor_category: EFFECT_CATEGORY,
                        operation: EFFECT_APPLY_OPERATION,
                        component_id: effect_id.clone(),
                        menu_category: format!("Image Effects / {effect_category}"),
                        display_kind: "Effect",
                        qa_id: format!("node_editor.menu.create.effect:{effect_id}"),
                        request: NodeCreateRequest::Effect(effect_id),
                        extra_keywords: vec![effect_name, effect_category, "image".to_string()],
                    },
                )
            }),
    );
    items
}

fn create_operation_node_for_request(
    request: &NodeCreateRequest,
    plugin_manager: &PluginManager,
) -> Option<Node> {
    let result = match request {
        NodeCreateRequest::Style(component_id) => {
            plugin_manager.create_style_operation_node(component_id)
        }
        NodeCreateRequest::Effector(component_id) => {
            plugin_manager.create_effector_operation_node(component_id)
        }
        NodeCreateRequest::Decorator(component_id) => {
            plugin_manager.create_decorator_operation_node(component_id)
        }
        NodeCreateRequest::Effect(effect_id) => {
            plugin_manager.create_effect_operation_node(effect_id)
        }
        NodeCreateRequest::Merge => return Some(Node::new_merge("Merge")),
        NodeCreateRequest::TimeModulo => return Some(Node::new_time_modulo("Time Modulo")),
        NodeCreateRequest::Text
        | NodeCreateRequest::Solid
        | NodeCreateRequest::Shape
        | NodeCreateRequest::SkSL
        | NodeCreateRequest::Clip
        | NodeCreateRequest::Track
        | NodeCreateRequest::Composition => return None,
    };
    match result {
        Ok(node) => Some(node),
        Err(error) => {
            log::warn!("Cannot prepare operation Node for wire insertion: {error}");
            None
        }
    }
}

fn wire_splice_menu_items(
    project: &Project,
    connection_id: Uuid,
    plugin_manager: &PluginManager,
) -> Vec<SearchableItem<NodeCreateRequest>> {
    node_create_menu_items(plugin_manager)
        .into_iter()
        .filter_map(|mut item| {
            let node = create_operation_node_for_request(&item.value, plugin_manager)?;
            let node_id = node.id;
            let mut probe = project.clone();
            probe.add_node(node);
            if !node_can_splice_connection(&probe, connection_id, node_id) {
                return None;
            }
            let suffix = item
                .qa_id
                .as_deref()
                .and_then(|id| id.strip_prefix("node_editor.menu.create."))
                .unwrap_or(item.value.qa_kind());
            item.qa_id = Some(format!("node_editor.wire_menu.operation.{suffix}"));
            item.qa_metadata = Some(serde_json::json!({
                "action": "splice",
                "connection_id": connection_id,
                "kind": item.value.qa_kind(),
            }));
            Some(item)
        })
        .collect()
}

const AUTHORED_BLEND_MODES: [BlendMode; 5] = [
    BlendMode::Normal,
    BlendMode::Add,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
];

fn blend_mode_label(blend_mode: BlendMode) -> &'static str {
    match blend_mode {
        BlendMode::Normal => "Normal",
        BlendMode::Add => "Add",
        BlendMode::Multiply => "Multiply",
        BlendMode::Screen => "Screen",
        BlendMode::Overlay => "Overlay",
    }
}

fn blend_mode_qa_key(blend_mode: BlendMode) -> &'static str {
    match blend_mode {
        BlendMode::Normal => "normal",
        BlendMode::Add => "add",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MergeLayerRow {
    merge_id: Uuid,
    connection_id: Uuid,
    source: PortAddress,
    source_label: String,
    authored_order: i64,
    authored_blend_mode: BlendMode,
    authored_blend_available: bool,
    back_to_front_index: usize,
    layer_count: usize,
}

impl MergeLayerRow {
    fn qa_metadata(&self, extra: Option<serde_json::Value>) -> serde_json::Value {
        let source_kind = match self.source.owner {
            PortOwner::Composition(_) => "composition",
            PortOwner::Track(_) => "track",
            PortOwner::Clip(_) => "clip",
            PortOwner::Node(_) => "node",
        };
        let mut metadata = serde_json::json!({
            "merge_id": self.merge_id,
            "connection_id": self.connection_id,
            "back_to_front_index": self.back_to_front_index,
            "layer_count": self.layer_count,
            "authored_order": self.authored_order,
            "authored_blend_mode": blend_mode_qa_key(self.authored_blend_mode),
            "authored_blend_available": self.authored_blend_available,
            "source": {
                "owner": qa_container_key(self.source.owner),
                "kind": source_kind,
                "port": self.source.port,
                "label": self.source_label,
                "full_name_available_on_hover": true,
            },
            "order_semantics": "back_to_front",
            "blend_ownership": "connection",
            "control_lane": "merge_body",
            "runtime_first_produced_may_be_normal": true,
        });
        if let (Some(target), Some(serde_json::Value::Object(extra))) =
            (metadata.as_object_mut(), extra)
        {
            target.extend(extra);
        }
        metadata
    }
}

fn merge_layer_source_label(project: &Project, owner: PortOwner) -> String {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| format!("Composition · {}", composition.name))
            .unwrap_or_else(|| "Missing Composition".to_string()),
        PortOwner::Track(id) => project
            .get_track(id)
            .map(|track| format!("Track · {}", track.name))
            .unwrap_or_else(|| "Missing Track".to_string()),
        PortOwner::Clip(id) => project
            .get_clip(id)
            .map(|clip| format!("Clip · {}", clip.name))
            .unwrap_or_else(|| "Missing Clip".to_string()),
        PortOwner::Node(id) => project
            .get_node(id)
            .map(|node| format!("Node · {}", node.name))
            .unwrap_or_else(|| "Missing Node".to_string()),
    }
}

fn merge_layer_rows(project: &Project, merge_id: Uuid) -> Vec<MergeLayerRow> {
    let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    let layer_count = connections.len();
    connections
        .into_iter()
        .enumerate()
        .map(|(back_to_front_index, connection)| MergeLayerRow {
            merge_id,
            connection_id: connection.id,
            source: connection.from.clone(),
            source_label: merge_layer_source_label(project, connection.from.owner),
            authored_order: connection.order,
            authored_blend_mode: connection.blend_mode,
            authored_blend_available: connection_supports_authored_blend(project, connection),
            back_to_front_index,
            layer_count,
        })
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "QA registration keeps semantic identity and transformed geometry explicit"
)]
fn register_merge_layer_component(
    id: String,
    component_type: &str,
    graph_rect: egui::Rect,
    enabled: bool,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    mut metadata: serde_json::Value,
) {
    let unclipped_rect = to_global * graph_rect;
    let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert(
            "unclipped_rect".to_string(),
            qa_rect_metadata(unclipped_rect),
        );
        metadata.insert(
            "visible_in_canvas".to_string(),
            serde_json::Value::Bool(rect.is_positive()),
        );
    }
    #[cfg(test)]
    capture_test_rect(&id, rect);
    crate::qa::register_component_with_metadata(id, component_type, rect, enabled, Some(metadata));
}

fn register_merge_layer_popup_component(
    id: String,
    component_type: &str,
    screen_rect: egui::Rect,
    enabled: bool,
    popup_clip: egui::Rect,
    mut metadata: serde_json::Value,
) {
    let rect = clipped_qa_rect(screen_rect, popup_clip);
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert("unclipped_rect".to_string(), qa_rect_metadata(screen_rect));
        metadata.insert("popup_clip_rect".to_string(), qa_rect_metadata(popup_clip));
        metadata.insert(
            "visible_in_popup".to_string(),
            serde_json::Value::Bool(rect.is_positive()),
        );
        metadata.insert(
            "coordinate_space".to_string(),
            serde_json::Value::String("screen_points".to_string()),
        );
    }
    #[cfg(test)]
    {
        capture_test_rect(&id, rect);
        capture_test_metadata(&id, &metadata);
    }
    crate::qa::register_component_with_metadata(id, component_type, rect, enabled, Some(metadata));
}

fn connection_supports_authored_blend(
    project: &Project,
    connection: &library::model::project::ProjectConnection,
) -> bool {
    let source_is_image = project
        .port_definition(&connection.from, PortDirection::Output)
        .is_some_and(|definition| definition.data_type == PortDataType::Image);
    let target_is_merge_images = connection.to.port == library::model::project::MERGE_IMAGES_PORT
        && matches!(
            connection.to.owner,
            PortOwner::Node(node_id)
                if project
                    .get_node(node_id)
                    .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
        )
        && project
            .port_definition(&connection.to, PortDirection::Input)
            .is_some_and(|definition| {
                definition.data_type == PortDataType::Image
                    && definition.multiplicity == PortMultiplicity::Variadic
            });
    source_is_image && target_is_merge_images
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WireOrderMenuState {
    back_to_front_index: usize,
    layer_count: usize,
}

fn wire_order_menu_state(
    project: &Project,
    connection: &library::model::project::ProjectConnection,
) -> Option<WireOrderMenuState> {
    wire_order_menu_states(project).get(&connection.id).copied()
}

fn wire_order_menu_states(project: &Project) -> HashMap<Uuid, WireOrderMenuState> {
    let mut groups = HashMap::<PortAddress, Vec<(i64, Uuid)>>::new();
    for connection in &project.connections {
        let is_variadic = project
            .port_definition(&connection.to, PortDirection::Input)
            .is_some_and(|definition| definition.multiplicity == PortMultiplicity::Variadic);
        if is_variadic {
            groups
                .entry(connection.to.clone())
                .or_default()
                .push((connection.order, connection.id));
        }
    }

    let mut states = HashMap::new();
    for siblings in groups.values_mut() {
        siblings.sort_by_key(|(order, id)| (*order, *id));
        let layer_count = siblings.len();
        for (back_to_front_index, (_, connection_id)) in siblings.iter().enumerate() {
            states.insert(
                *connection_id,
                WireOrderMenuState {
                    back_to_front_index,
                    layer_count,
                },
            );
        }
    }
    states
}

fn wire_order_qa_metadata(
    connection: &library::model::project::ProjectConnection,
    order: WireOrderMenuState,
    direction: &str,
    target_index: Option<usize>,
) -> serde_json::Value {
    serde_json::json!({
        "action": "reorder",
        "connection_id": connection.id,
        "direction": direction,
        "back_to_front_index": order.back_to_front_index,
        "authored_order": connection.order,
        "layer_count": order.layer_count,
        "target_back_to_front_index": target_index,
        "authored_blend_mode": blend_mode_qa_key(connection.blend_mode),
    })
}

fn show_wire_context_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    project: &Project,
    plugin_manager: &PluginManager,
    composition_id: Uuid,
    to_global: egui::emath::TSTransform,
) -> Option<QueuedNodeEdit> {
    let target = state.wire_context_menu.as_ref()?.target;
    if let NodeEditorEditableWire::OutputBinding { owner, node_id } = target {
        return show_output_binding_wire_context_menu(ui, state, project, owner, node_id);
    }
    let context = state.wire_context_menu.as_mut()?;
    let NodeEditorEditableWire::ProjectConnection { connection_id } = context.target else {
        return None;
    };
    let Some(connection) = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .cloned()
    else {
        state.wire_context_menu = None;
        return None;
    };

    let order_state = wire_order_menu_state(project, &connection);
    let authored_blend_available = connection_supports_authored_blend(project, &connection);
    let position = context.position;
    let graph_position = to_global.inverse() * position;
    let popup =
        searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
    let mut edit = None;
    let mut should_close = false;
    let response = egui::Area::new(egui::Id::new(("node_wire_context_menu", connection_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(popup.position)
        .show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_width(popup.width);
                ui.set_max_height(popup.max_height);
                if context.inserting {
                    let items = wire_splice_menu_items(project, connection_id, plugin_manager);
                    if items.is_empty() {
                        non_selectable_label(ui, "No compatible operations");
                    } else if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &format!(
                            "node_editor_wire_insert_menu:{connection_id}:{}",
                            context.open_time.to_bits()
                        ),
                        Some("node_editor.wire_menu.search"),
                        &items,
                    ) {
                        if let Some(node) =
                            create_operation_node_for_request(&request, plugin_manager)
                        {
                            edit = Some(QueuedNodeEdit::Atomic(NodeEdit::InsertNodeOnConnection {
                                connection_id,
                                node: Box::new(node),
                                position: graph_position,
                                composition_id,
                            }));
                        }
                        should_close = true;
                    }
                    return;
                }

                if let Some(order) = order_state {
                    let order_label = non_selectable_label(
                        ui,
                        format!(
                            "Layer Order · Back→Front {} / {}",
                            order.back_to_front_index + 1,
                            order.layer_count
                        ),
                    );
                    crate::qa::register_component_with_metadata(
                        format!("node_editor.wire_menu.order:{connection_id}"),
                        "node_editor_wire_order",
                        order_label.rect,
                        true,
                        Some(serde_json::json!({
                            "connection_id": connection_id,
                            "back_to_front_index": order.back_to_front_index,
                            "authored_order": connection.order,
                            "layer_count": order.layer_count,
                            "authored_blend_mode": blend_mode_qa_key(connection.blend_mode),
                        })),
                    );
                    ui.horizontal(|ui| {
                        let back_index = order.back_to_front_index.checked_sub(1);
                        let back = ui
                            .add_enabled(
                                back_index.is_some(),
                                egui::Button::new("Move Back"),
                            )
                            .on_hover_text("Move this Merge input one layer toward the back");
                        crate::qa::register_component_with_metadata(
                            format!("node_editor.wire_menu.order_back:{connection_id}"),
                            "node_editor_menu_item",
                            back.rect,
                            back.enabled(),
                            Some(wire_order_qa_metadata(
                                &connection,
                                order,
                                "back",
                                back_index,
                            )),
                        );
                        if back.clicked() {
                            edit = back_index.map(|new_order| {
                                QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                                    connection_id,
                                    new_order: new_order as i64,
                                })
                            });
                            should_close = true;
                        }

                        let front_index = (order.back_to_front_index + 1 < order.layer_count)
                            .then_some(order.back_to_front_index + 1);
                        let front = ui
                            .add_enabled(
                                front_index.is_some(),
                                egui::Button::new("Move Front"),
                            )
                            .on_hover_text("Move this Merge input one layer toward the front");
                        crate::qa::register_component_with_metadata(
                            format!("node_editor.wire_menu.order_front:{connection_id}"),
                            "node_editor_menu_item",
                            front.rect,
                            front.enabled(),
                            Some(wire_order_qa_metadata(
                                &connection,
                                order,
                                "front",
                                front_index,
                            )),
                        );
                        if front.clicked() {
                            edit = front_index.map(|new_order| {
                                QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                                    connection_id,
                                    new_order: new_order as i64,
                                })
                            });
                            should_close = true;
                        }
                    });
                }

                if authored_blend_available {
                    ui.separator();
                    let blend_label = non_selectable_label(
                        ui,
                        format!(
                            "Authored Blend · {}",
                            blend_mode_label(connection.blend_mode)
                        ),
                    )
                    .on_hover_text(
                        "This value belongs to the Merge input wire, not to the source Node",
                    );
                    crate::qa::register_component_with_metadata(
                        format!("node_editor.wire_menu.authored_blend:{connection_id}"),
                        "node_editor_wire_authored_blend",
                        blend_label.rect,
                        true,
                        Some(serde_json::json!({
                            "connection_id": connection_id,
                            "authored_blend_mode": blend_mode_qa_key(connection.blend_mode),
                            "runtime_note": "The first produced Merge layer composites as Normal; the wire keeps its authored blend.",
                        })),
                    );
                    for blend_mode in AUTHORED_BLEND_MODES {
                        let selected = blend_mode == connection.blend_mode;
                        let blend = ui
                            .add_enabled(
                                !selected,
                                egui::Button::selectable(
                                    selected,
                                    format!("Blend · {}", blend_mode_label(blend_mode)),
                                )
                                .frame(false),
                            )
                            .on_hover_text(
                                "Authored on this wire. The first produced runtime layer may composite as Normal.",
                            );
                        crate::qa::register_component_with_metadata(
                            format!(
                                "node_editor.wire_menu.blend.{}:{connection_id}",
                                blend_mode_qa_key(blend_mode)
                            ),
                            "node_editor_menu_item",
                            blend.rect,
                            blend.enabled(),
                            Some(serde_json::json!({
                                "action": "set_authored_blend",
                                "connection_id": connection_id,
                                "blend_mode": blend_mode_qa_key(blend_mode),
                                "selected": selected,
                                "runtime_first_produced_may_be_normal": true,
                            })),
                        );
                        if blend.clicked() {
                            edit = Some(QueuedNodeEdit::Atomic(
                                NodeEdit::SetConnectionBlendMode {
                                    connection_id,
                                    blend_mode,
                                },
                            ));
                            should_close = true;
                        }
                    }
                    non_selectable_label(
                        ui,
                        egui::RichText::new(
                            "Runtime: first produced Merge layer composites as Normal",
                        )
                        .small()
                        .weak(),
                    );
                }

                if order_state.is_some() || authored_blend_available {
                    ui.separator();
                }

                let delete = ui.button("Delete Wire");
                crate::qa::register_component_with_metadata(
                    format!("node_editor.wire_menu.delete:{connection_id}"),
                    "node_editor_menu_item",
                    delete.rect,
                    delete.enabled(),
                    Some(serde_json::json!({
                        "action": "delete",
                        "connection_id": connection_id,
                    })),
                );
                if delete.clicked() {
                    edit = Some(QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires {
                        wires: vec![NodeEditorEditableWire::ProjectConnection {
                            connection_id,
                        }],
                    }));
                    should_close = true;
                }

                let insert = ui.button("Insert Operation…");
                crate::qa::register_component_with_metadata(
                    format!("node_editor.wire_menu.insert:{connection_id}"),
                    "node_editor_menu_item",
                    insert.rect,
                    insert.enabled(),
                    Some(serde_json::json!({
                        "action": "open_splice_menu",
                        "connection_id": connection_id,
                    })),
                );
                if insert.clicked() {
                    context.inserting = true;
                }
            });
        });
    crate::qa::register_component_with_metadata(
        format!("node_editor.wire_menu:{connection_id}"),
        "node_editor_wire_menu",
        response.response.rect,
        true,
        Some(serde_json::json!({
            "connection_id": connection_id,
            "mode": if context.inserting { "insert" } else { "commands" },
            "order": order_state.map(|order| serde_json::json!({
                "back_to_front_index": order.back_to_front_index,
                "authored_order": connection.order,
                "layer_count": order.layer_count,
                "can_move_back": order.back_to_front_index > 0,
                "can_move_front": order.back_to_front_index + 1 < order.layer_count,
                "authored_blend_mode": blend_mode_qa_key(connection.blend_mode),
            })),
            "authored_blend": {
                "available": authored_blend_available,
                "mode": authored_blend_available.then(|| blend_mode_qa_key(connection.blend_mode)),
                "runtime_first_produced_may_be_normal": authored_blend_available,
            },
        })),
    );

    if ui.input(|input| input.pointer.any_click())
        && ui.input(|input| input.time) - context.open_time > 0.2
        && ui
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|pointer| !response.response.rect.contains(pointer))
    {
        should_close = true;
    }
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }
    if should_close {
        state.wire_context_menu = None;
        state.selected_connection_id = None;
    }
    edit
}

fn show_output_binding_wire_context_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    project: &Project,
    owner: PortOwner,
    node_id: Uuid,
) -> Option<QueuedNodeEdit> {
    let target = NodeEditorEditableWire::OutputBinding { owner, node_id };
    if !editable_wire_is_current(project, target) {
        state.wire_context_menu = None;
        return None;
    }
    let context = state.wire_context_menu.as_mut()?;
    let position = context.position;
    let open_time = context.open_time;
    let stable_key = editable_wire_stable_key(target);
    let mut edit = None;
    let mut should_close = false;
    let response = egui::Area::new(egui::Id::new(("node_wire_context_menu", target)))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(240.0);
                non_selectable_label(ui, "Container Output Binding")
                    .on_hover_text("This authored wire selects the Node rendered by the container");
                let delete = ui
                    .button("Delete Wire")
                    .on_hover_text("Clear the container output binding without deleting the Node");
                crate::qa::register_component_with_metadata(
                    format!("node_editor.wire_menu.delete:{stable_key}"),
                    "node_editor_menu_item",
                    delete.rect,
                    delete.enabled(),
                    Some(serde_json::json!({
                        "action": "clear_output_binding",
                        "kind": "output_binding",
                        "owner": qa_container_key(owner),
                        "node_id": node_id,
                    })),
                );
                if delete.clicked() {
                    edit = Some(QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires {
                        wires: vec![target],
                    }));
                    should_close = true;
                }
            });
        });
    crate::qa::register_component_with_metadata(
        format!("node_editor.wire_menu:{stable_key}"),
        "node_editor_wire_menu",
        response.response.rect,
        true,
        Some(serde_json::json!({
            "kind": "output_binding",
            "owner": qa_container_key(owner),
            "node_id": node_id,
            "mode": "commands",
            "editable": true,
        })),
    );

    if ui.input(|input| input.pointer.any_click())
        && ui.input(|input| input.time) - open_time > 0.2
        && ui
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|pointer| !response.response.rect.contains(pointer))
    {
        should_close = true;
    }
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }
    if should_close {
        state.wire_context_menu = None;
        state.selected_connection_id = None;
    }
    edit
}

type CreateAction = Box<dyn FnOnce(&mut Project) -> bool>;

fn create_action_for_request(
    request: NodeCreateRequest,
    project_service: &EditorService,
    canvas_size: (u64, u64),
    graph_position: egui::Pos2,
    comp_id: Uuid,
) -> Option<CreateAction> {
    let plugin_manager = project_service.get_plugin_manager();
    match request {
        NodeCreateRequest::Text => match project_service.create_text_node(
            "Hello World",
            library::editor::project_service::DEFAULT_TEXT_FONT,
            canvas_size.0,
            canvas_size.1,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create Text Node: {error}");
                None
            }
        },
        NodeCreateRequest::Solid => match project_service.create_solid_node(
            library::model::frame::color::Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            canvas_size.0,
            canvas_size.1,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create Solid Node: {error}");
                None
            }
        },
        NodeCreateRequest::Shape => match project_service.create_shape_node(
            library::editor::project_service::DEFAULT_SHAPE_PATH,
            canvas_size.0,
            canvas_size.1,
            100,
            100,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create Shape Node: {error}");
                None
            }
        },
        NodeCreateRequest::SkSL => match project_service.create_sksl_node(
            library::editor::project_service::DEFAULT_SKSL_SHADER,
            canvas_size.0,
            canvas_size.1,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create SkSL Node: {error}");
                None
            }
        },
        NodeCreateRequest::TimeModulo => Some(Box::new(move |project| {
            create_prebuilt_node(
                project,
                graph_position,
                Node::new_time_modulo("Time Modulo"),
                comp_id,
            )
        })),
        NodeCreateRequest::Style(component_id) => {
            match plugin_manager.create_style_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Style Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Effector(component_id) => {
            match plugin_manager.create_effector_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Effector Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Decorator(component_id) => {
            match plugin_manager.create_decorator_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Decorator Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Effect(effect_id) => {
            match plugin_manager.create_effect_operation_node(&effect_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Effect Node {effect_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Merge => Some(Box::new(move |project| {
            create_merge_node(project, graph_position, comp_id)
        })),
        NodeCreateRequest::Clip => Some(Box::new(move |project| {
            create_clip_at_free_slot(project, graph_position, comp_id, "Clip").is_some()
        })),
        NodeCreateRequest::Track => Some(Box::new(move |project| {
            create_track_at_free_slot(project, graph_position, comp_id, "Track").is_some()
        })),
        NodeCreateRequest::Composition => Some(Box::new(move |project| {
            create_composition_node(project, graph_position, comp_id)
        })),
    }
}

struct NodeContextMenuFrame<'a> {
    project_lock: &'a Arc<RwLock<Project>>,
    project_service: &'a EditorService,
    comp_id: Uuid,
    exclusion_rects: &'a [egui::Rect],
    to_global: egui::emath::TSTransform,
    suppress_secondary_click: bool,
}

fn handle_context_menu(
    ui: &mut egui::Ui,
    state: &mut Option<ContextMenuState>,
    frame: NodeContextMenuFrame<'_>,
) -> bool {
    let canvas_size = frame
        .project_lock
        .read()
        .ok()
        .and_then(|project| {
            project
                .get_composition(frame.comp_id)
                .map(|composition| (composition.width, composition.height))
        })
        .unwrap_or((1920, 1080));
    let from_global = frame.to_global.inverse();
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.hover_pos(),
            input.time,
        )
    });
    update_global_context_menu_for_secondary_click(
        state,
        secondary_clicked && !frame.suppress_secondary_click,
        pointer_position,
        ui.min_rect(),
        frame.exclusion_rects,
        frame.to_global,
        open_time,
    );

    let mut should_close = false;
    let mut action: Option<CreateAction> = None;

    if let Some(context) = state {
        let position = context.position;
        let graph_position = from_global * position;
        let popup =
            searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
        let response = egui::Area::new(egui::Id::new("node_ctx_menu"))
            .fixed_pos(popup.position)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    let plugin_manager = frame.project_service.get_plugin_manager();
                    ui.set_width(popup.width);
                    ui.set_max_height(popup.max_height);
                    let items = node_create_menu_items(plugin_manager.as_ref());
                    let menu_id = format!("node_editor_add_menu:{}", context.open_time.to_bits());
                    if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &menu_id,
                        Some("node_editor.menu.search"),
                        &items,
                    ) {
                        action = create_action_for_request(
                            request,
                            frame.project_service,
                            canvas_size,
                            graph_position,
                            frame.comp_id,
                        );
                        should_close = true;
                    }
                });
            });

        if ui.input(|input| input.pointer.any_click())
            && ui.input(|input| input.time) - context.open_time > 0.2
        {
            if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
                if !response.response.rect.contains(pointer) {
                    should_close = true;
                }
            }
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
    }

    let mut changed = false;
    if let Some(action) = action {
        if let Ok(mut project) = frame.project_lock.write() {
            changed = action(&mut project);
        }
    }
    if should_close {
        *state = None;
    }
    changed
}

fn update_global_context_menu_for_secondary_click(
    state: &mut Option<ContextMenuState>,
    secondary_clicked: bool,
    pointer_position: Option<egui::Pos2>,
    canvas_rect: egui::Rect,
    exclusion_rects: &[egui::Rect],
    to_global: egui::emath::TSTransform,
    open_time: f64,
) {
    if !secondary_clicked {
        return;
    }
    let Some(position) = pointer_position.filter(|position| canvas_rect.contains(*position)) else {
        return;
    };
    let graph_position = to_global.inverse() * position;
    if exclusion_rects
        .iter()
        .any(|rect| rect.contains(graph_position))
    {
        // A Snarl item owns this gesture. Also close a stale Create menu so a
        // Node/container menu and the global menu cannot remain visible at the
        // same time after a secondary click.
        *state = None;
        return;
    }
    *state = Some(ContextMenuState::new(position, open_time));
}

fn push_history_snapshot(
    project_lock: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    if let Ok(project) = project_lock.read() {
        history_manager.push_project_state(project.clone());
    }
}

#[cfg(test)]
fn available_effector_menu_entries(plugin_manager: &PluginManager) -> Vec<(String, String)> {
    let mut entries = plugin_manager
        .get_available_effectors()
        .into_iter()
        .filter_map(|component_id| {
            match plugin_manager.operation_descriptor(
                EFFECTOR_CATEGORY,
                &component_id,
                EFFECTOR_APPLY_OPERATION,
            ) {
                Ok(descriptor) => Some((component_id, descriptor.label().to_string())),
                Err(error) => {
                    log::warn!("Cannot expose Effector {component_id} in the Node Editor: {error}");
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    entries
}

fn insert_prebuilt_graph(
    project: &mut Project,
    desired: egui::Pos2,
    mut graph: NodeGraphBundle,
    composition_id: Uuid,
) -> bool {
    let Some(container) = node_container_at_position(project, composition_id, desired) else {
        return false;
    };
    let Some((container_position, container_size, existing_node_ids)) =
        container_geometry(project, container)
    else {
        return false;
    };

    // Factory coordinates are only hints. Plugin-backed Nodes may have many
    // more property ports than a factory can anticipate, so lay the detached
    // graph out from its canonical connections and the same conservative
    // card measurements used by the rest of this editor before placement.
    // This is deliberately an app concern: the library factory remains a
    // renderer/UI-independent graph constructor.
    layout_detached_node_graph(project, &mut graph);

    // `output_node_id` identifies the consumer/sink within a detached factory
    // graph. It is useful for creating a brand-new Clip, but ordinary Add in
    // an existing container must never silently replace that container's
    // explicit output binding. Setting an output remains a separate command.
    graph.output_node_id = None;

    // Measure with the same canonical port-derived estimator used for
    // existing Nodes. A temporary Project keeps this layout calculation out
    // of the authoritative model until the atomic insert succeeds.
    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }
    let mut graph_bounds = egui::Rect::NOTHING;
    for node in &graph.nodes {
        let rect = egui::Rect::from_min_size(
            egui::pos2(node.ui_position[0], node.ui_position[1]),
            estimated_node_size(&measurement_project, node.id),
        );
        graph_bounds = graph_bounds.union(rect);
    }
    if !graph_bounds.is_finite() || !graph_bounds.is_positive() {
        return false;
    }

    let content_left = container_position[0]
        + match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_LEFT,
            NodeContainer::Track(_) | NodeContainer::Clip(_) => AUTO_LAYOUT_TRACK_LEFT,
        };
    let content_top = container_position[1]
        + match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
            NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
            NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
        };
    let max_x = (container_position[0] + container_size[0]
        - AUTO_LAYOUT_TRACK_RIGHT
        - graph_bounds.width())
    .max(content_left);
    let anchor = egui::pos2(
        desired.x.max(content_left).clamp(content_left, max_x),
        desired.y.max(content_top),
    );
    let mut candidate = egui::Rect::from_min_size(anchor, graph_bounds.size());
    let mut occupied = existing_node_ids
        .iter()
        .filter_map(|node_id| estimated_node_rect(project, *node_id))
        .collect::<Vec<_>>();
    occupied.extend(immediate_child_rects(
        project,
        &AutoLayoutPlan::default(),
        container,
    ));
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| rects_are_closer_than(candidate, **other, DETACHED_GRAPH_NODE_GAP))
            .map(|other| other.bottom() + DETACHED_GRAPH_NODE_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(anchor.x, next_y), graph_bounds.size());
    }

    let translation = candidate.min - graph_bounds.min;
    for node in &mut graph.nodes {
        node.ui_position[0] += translation.x;
        node.ui_position[1] += translation.y;
    }
    if let Err(error) = project.insert_node_graph(container, graph) {
        log::warn!("Cannot insert Node graph into {container:?}: {error}");
        return false;
    }

    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_collapsed = false;
    }
    ensure_container_hierarchy_contains(project, container, candidate);
    true
}

fn layout_detached_node_graph(project: &Project, graph: &mut NodeGraphBundle) {
    if graph.nodes.is_empty() {
        return;
    }

    let node_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let node_set = node_ids.iter().copied().collect::<HashSet<_>>();
    let mut edges = graph
        .connections
        .iter()
        .filter_map(|connection| {
            let (PortOwner::Node(from), PortOwner::Node(to)) =
                (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            (node_set.contains(&from) && node_set.contains(&to)).then_some((from, to))
        })
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    let ranks = rank_nodes_by_scc(&node_ids, &edges);

    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }

    // Preserve factory order within a rank. Connection order has semantic
    // meaning for multi-input Style ports, and matching that order visually
    // makes Fill-before-Stroke immediately legible.
    let mut columns = BTreeMap::<usize, Vec<Uuid>>::new();
    for node_id in &node_ids {
        columns
            .entry(ranks.get(node_id).copied().unwrap_or_default())
            .or_default()
            .push(*node_id);
    }

    // Half-open geometry would allow an exact 24 px gap. egui Rect
    // intersection treats touching expanded edges as an intersection, so a
    // tiny extra margin keeps this layout out of the reflow detector while
    // preserving the intended 24 px visual rhythm.
    let gap = DETACHED_GRAPH_NODE_GAP;
    let mut column_x = BTreeMap::<usize, f32>::new();
    let mut x = 0.0;
    for (rank, node_ids) in &columns {
        column_x.insert(*rank, x);
        let width = node_ids
            .iter()
            .map(|node_id| estimated_node_size(&measurement_project, *node_id).x)
            .max_by(f32::total_cmp)
            .unwrap_or_default();
        x += width + gap;
    }

    let mut positions = HashMap::<Uuid, [f32; 2]>::new();
    for (rank, node_ids) in columns {
        let mut y = 0.0;
        for node_id in node_ids {
            positions.insert(node_id, [column_x[&rank], y]);
            y += estimated_node_size(&measurement_project, node_id).y + gap;
        }
    }
    for node in &mut graph.nodes {
        if let Some(position) = positions.get(&node.id) {
            node.ui_position = *position;
        }
    }
}

fn create_prebuilt_node(
    project: &mut Project,
    position: egui::Pos2,
    mut node: Node,
    comp_id: Uuid,
) -> bool {
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    project.add_node(node);
    if let Some(container) = attach_node_at_position(project, node_id, comp_id, position) {
        place_node_in_free_slot(project, node_id, container, position, &[]);
        true
    } else {
        project.remove_node(node_id);
        false
    }
}

fn create_composition_node(project: &mut Project, position: egui::Pos2, comp_id: Uuid) -> bool {
    let mut candidate = project.clone();
    let (composition, root) =
        library::model::Composition::new("Nested Comp", 1920, 1080, 30.0, 10.0);
    let nested_id = composition.id;
    candidate.add_track(root);
    candidate.add_composition(composition);

    let mut node = Node::new_reference(
        "Container",
        library::model::ReferenceContent {
            target_id: nested_id,
            sync_global_time: false,
        },
    );
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    candidate.add_node(node);
    if let Some(container) = attach_node_at_position(&mut candidate, node_id, comp_id, position) {
        place_node_in_free_slot(&mut candidate, node_id, container, position, &[]);
        *project = candidate;
        true
    } else {
        false
    }
}

fn create_merge_node(project: &mut Project, position: egui::Pos2, comp_id: Uuid) -> bool {
    create_prebuilt_node(project, position, Node::new_merge("Merge"), comp_id)
}

fn attach_node_at_position(
    project: &mut Project,
    node_id: Uuid,
    comp_id: Uuid,
    position: egui::Pos2,
) -> Option<NodeContainer> {
    let container = node_container_at_position(project, comp_id, position)?;
    if let Err(error) = project.attach_node_to_container(container, node_id) {
        log::warn!("Cannot add Node to {container:?}: {error}");
        return None;
    }
    // A collapsed root has no visible expanded parent to receive a Node.
    // Expand it after the atomic attachment so every successfully created Node
    // is immediately projected by `build_snarl`.
    if let NodeContainer::Composition(composition_id) = container {
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_collapsed = false;
        }
    }
    Some(container)
}

fn place_node_in_free_slot(
    project: &mut Project,
    node_id: Uuid,
    container: NodeContainer,
    desired: egui::Pos2,
    dependencies: &[Uuid],
) -> Option<egui::Pos2> {
    let (container_position, container_size, node_ids) = container_geometry(project, container)?;
    let node_size = estimated_node_size(project, node_id);
    let dependency_anchor = dependencies
        .iter()
        .filter_map(|dependency_id| project.get_node(*dependency_id))
        .map(|dependency| dependency.ui_position)
        .collect::<Vec<_>>();
    let mut anchor = if dependency_anchor.is_empty() {
        desired
    } else {
        let count = dependency_anchor.len() as f32;
        egui::pos2(
            dependency_anchor
                .iter()
                .map(|position| position[0])
                .sum::<f32>()
                / count
                + estimated_node_width()
                + AUTO_LAYOUT_COLUMN_GAP,
            dependency_anchor
                .iter()
                .map(|position| position[1])
                .sum::<f32>()
                / count,
        )
    };
    let min = egui::pos2(
        container_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        container_position[1]
            + match container {
                NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
                NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
                NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
            },
    );
    let max_x = (container_position[0] + container_size[0] - AUTO_LAYOUT_TRACK_RIGHT - node_size.x)
        .max(min.x);
    // A dependency-derived position expresses graph order, so preserve it and
    // let `ensure_container_hierarchy_contains` grow the owning containers.
    // Clamping it to a container that is still sized for the old children can
    // place the new dependent Node to the *left* of its source. Pointer-based
    // placement, on the other hand, should remain within the current bounds.
    anchor.x = if dependency_anchor.is_empty() {
        anchor.x.clamp(min.x, max_x)
    } else {
        anchor.x.max(min.x)
    };
    anchor.y = anchor.y.max(min.y);

    let mut occupied = node_ids
        .iter()
        .filter(|child_id| **child_id != node_id)
        .filter_map(|child_id| {
            let child = project.get_node(*child_id)?;
            Some(egui::Rect::from_min_size(
                egui::pos2(child.ui_position[0], child.ui_position[1]),
                estimated_node_size(project, *child_id),
            ))
        })
        .collect::<Vec<_>>();
    occupied.extend(immediate_child_rects(
        project,
        &AutoLayoutPlan::default(),
        container,
    ));
    let mut candidate = egui::Rect::from_min_size(anchor, node_size);
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| candidate.expand(4.0).intersects(**other))
            .map(|other| other.bottom() + AUTO_LAYOUT_ROW_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(anchor.x, next_y), node_size);
    }

    project.get_node_mut(node_id)?.ui_position = [candidate.min.x, candidate.min.y];
    ensure_container_hierarchy_contains(project, container, candidate);
    Some(candidate.min)
}

fn container_geometry(
    project: &Project,
    container: NodeContainer,
) -> Option<([f32; 2], [f32; 2], Vec<Uuid>)> {
    match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
        NodeContainer::Clip(id) => project
            .get_clip(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
    }
}

/// Resolves the deepest visible container chrome for pointer-only creation and
/// insertion callers. Those callers intentionally accept headers. Geometry
/// reparenting uses `reparent_container_geometries` and content-only target
/// evaluation instead, so neither a collapsed header nor its stored body can
/// acquire an existing Node.
fn node_container_at_position(
    project: &Project,
    composition_id: Uuid,
    position: egui::Pos2,
) -> Option<NodeContainer> {
    let composition = project.get_composition(composition_id)?;
    if composition.ui_collapsed {
        return Some(NodeContainer::Composition(composition_id));
    }
    for track_id in composition.track_ids.iter().rev() {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        if !container_visual(project, PortOwner::Track(*track_id))
            .is_some_and(|visual| visual.rect().contains(position))
        {
            continue;
        }
        if !track.ui_collapsed {
            for clip_id in track.clip_ids.iter().rev() {
                if container_visual(project, PortOwner::Clip(*clip_id))
                    .is_some_and(|visual| visual.rect().contains(position))
                {
                    return Some(NodeContainer::Clip(*clip_id));
                }
            }
        }
        return Some(NodeContainer::Track(*track_id));
    }
    Some(NodeContainer::Composition(composition_id))
}

#[derive(Clone, Copy, Debug)]
struct ReparentContainerGeometry {
    container: NodeContainer,
    visible_rect: egui::Rect,
    content_rect: Option<egui::Rect>,
    depth: u8,
    stacking_order: usize,
    collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReparentTargetKind {
    Geometry,
    RetainedOrigin,
    RootFallback,
}

impl ReparentTargetKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Geometry => "geometry",
            Self::RetainedOrigin => "retained_origin",
            Self::RootFallback => "root_fallback",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ReparentTargetEvaluation {
    container: NodeContainer,
    visible_rect: egui::Rect,
    content_rect: egui::Rect,
    depth: u8,
    stacking_order: usize,
    overlap_ratio: f32,
    center_inside: bool,
    pointer_inside: bool,
    root_fallback: bool,
    score: f32,
    kind: ReparentTargetKind,
}

#[derive(Clone, Copy, Debug)]
struct NodeDropIntent {
    node_id: Uuid,
    final_rect: egui::Rect,
    target: ReparentTargetEvaluation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReparentReleaseOutcome {
    NoIntent,
    Applied,
    Rejected,
}

fn reparent_container_geometries(
    project: &Project,
    composition_id: Uuid,
) -> Vec<ReparentContainerGeometry> {
    let Some(composition) = project.get_composition(composition_id) else {
        return Vec::new();
    };
    let composition_rect = container_rect(composition.ui_position, composition.ui_size);
    let mut geometries = vec![ReparentContainerGeometry {
        container: NodeContainer::Composition(composition_id),
        visible_rect: if composition.ui_collapsed {
            egui::Rect::from_min_size(
                composition_rect.min,
                egui::vec2(composition_rect.width(), CONTAINER_HEADER_HEIGHT),
            )
        } else {
            composition_rect
        },
        content_rect: (!composition.ui_collapsed)
            .then(|| composition_content_rect(composition_rect)),
        depth: 0,
        stacking_order: 0,
        collapsed: composition.ui_collapsed,
    }];
    if composition.ui_collapsed {
        return geometries;
    }

    let mut stacking_order = 1;
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        let visible_rect = if track.ui_collapsed {
            egui::Rect::from_min_size(
                track_rect.min,
                egui::vec2(track_rect.width(), CONTAINER_HEADER_HEIGHT),
            )
        } else {
            track_rect
        };
        geometries.push(ReparentContainerGeometry {
            container: NodeContainer::Track(*track_id),
            visible_rect,
            content_rect: (!track.ui_collapsed)
                .then(|| nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP)),
            depth: 1,
            stacking_order,
            collapsed: track.ui_collapsed,
        });
        stacking_order += 1;
        if track.ui_collapsed {
            continue;
        }
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            let visible_rect = if clip.ui_collapsed {
                egui::Rect::from_min_size(
                    clip_rect.min,
                    egui::vec2(clip_rect.width(), CONTAINER_HEADER_HEIGHT),
                )
            } else {
                clip_rect
            };
            geometries.push(ReparentContainerGeometry {
                container: NodeContainer::Clip(*clip_id),
                visible_rect,
                content_rect: (!clip.ui_collapsed)
                    .then(|| nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP)),
                depth: 2,
                stacking_order,
                collapsed: clip.ui_collapsed,
            });
            stacking_order += 1;
        }
    }
    geometries
}

fn rect_area(rect: egui::Rect) -> f32 {
    if rect.is_positive() {
        rect.width() * rect.height()
    } else {
        0.0
    }
}

fn overlap_ratio(outer: egui::Rect, inner: egui::Rect) -> f32 {
    let inner_area = rect_area(inner);
    if inner_area <= f32::EPSILON {
        return 0.0;
    }
    (rect_area(outer.intersect(inner)) / inner_area).clamp(0.0, 1.0)
}

fn evaluate_reparent_target(
    geometry: ReparentContainerGeometry,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let content_rect = geometry.content_rect?;
    if !content_rect.is_positive() || !node_rect.is_positive() {
        return None;
    }
    let overlap_ratio = overlap_ratio(content_rect, node_rect);
    let center_inside = content_rect.contains(node_rect.center());
    let pointer_inside = content_rect.contains(drop_point);
    let eligible =
        center_inside || pointer_inside && overlap_ratio >= NODE_REPARENT_POINTER_OVERLAP_THRESHOLD;
    if !eligible {
        return None;
    }
    let score = f32::from(geometry.depth) * 10_000.0
        + if center_inside { 1_000.0 } else { 0.0 }
        + if pointer_inside { 500.0 } else { 0.0 }
        + overlap_ratio * 100.0
        + geometry.stacking_order as f32 * 0.001;
    Some(ReparentTargetEvaluation {
        container: geometry.container,
        visible_rect: geometry.visible_rect,
        content_rect,
        depth: geometry.depth,
        stacking_order: geometry.stacking_order,
        overlap_ratio,
        center_inside,
        pointer_inside,
        root_fallback: false,
        score,
        kind: ReparentTargetKind::Geometry,
    })
}

fn deepest_legal_reparent_target(
    project: &Project,
    composition_id: Uuid,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let geometries = reparent_container_geometries(project, composition_id);
    let selected = geometries
        .iter()
        .copied()
        .filter_map(|geometry| evaluate_reparent_target(geometry, node_rect, drop_point))
        .max_by(|left, right| {
            left.depth
                .cmp(&right.depth)
                .then_with(|| left.center_inside.cmp(&right.center_inside))
                .then_with(|| left.pointer_inside.cmp(&right.pointer_inside))
                .then_with(|| left.overlap_ratio.total_cmp(&right.overlap_ratio))
                .then_with(|| left.stacking_order.cmp(&right.stacking_order))
        });
    if selected.is_some() {
        return selected;
    }

    // The Composition is the root graph owner. A deliberate drop beyond its
    // current bounds remains legal; the atomic reparent step expands the root
    // on all four sides so visual containment catches up without moving the
    // Node. Nested containers never receive this fallback.
    let root = geometries
        .into_iter()
        .find(|geometry| geometry.container == NodeContainer::Composition(composition_id))?;
    let content_rect = root.content_rect?;
    Some(ReparentTargetEvaluation {
        container: root.container,
        visible_rect: root.visible_rect,
        content_rect,
        depth: root.depth,
        stacking_order: root.stacking_order,
        overlap_ratio: overlap_ratio(content_rect, node_rect),
        center_inside: false,
        pointer_inside: false,
        root_fallback: true,
        score: -1.0,
        kind: ReparentTargetKind::RootFallback,
    })
}

fn node_drop_intents(
    project: &Project,
    composition_id: Uuid,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    final_positions: &HashMap<Uuid, [f32; 2]>,
    drop_point: egui::Pos2,
    canvas_scale: f32,
) -> Vec<NodeDropIntent> {
    let scale = sanitized_node_editor_scale(canvas_scale);
    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    let Some(primary_node_id) = gesture.primary_node_id else {
        return Vec::new();
    };
    let Some(primary_origin) = gesture.origins.get(&primary_node_id) else {
        return Vec::new();
    };
    let Some(primary_final_position) =
        final_positions.get(&primary_node_id).copied().or_else(|| {
            project
                .get_node(primary_node_id)
                .map(|node| node.ui_position)
        })
    else {
        return Vec::new();
    };
    let displacement = egui::vec2(
        primary_final_position[0] - primary_origin.position[0],
        primary_final_position[1] - primary_origin.position[1],
    );
    let Some(primary_rect) = rendered_node_rects.get(&primary_node_id).copied() else {
        return Vec::new();
    };
    if displacement.length() * scale < NODE_REPARENT_DRAG_THRESHOLD {
        return retained_origin_intents(
            project,
            composition_id,
            gesture,
            rendered_node_rects,
            drop_point,
        );
    }
    let Some(resolved_target) =
        deepest_legal_reparent_target(project, composition_id, primary_rect, drop_point)
    else {
        return Vec::new();
    };
    if resolved_target.container == primary_origin.container {
        if !resolved_target.root_fallback {
            return retained_origin_intents(
                project,
                composition_id,
                gesture,
                rendered_node_rects,
                drop_point,
            );
        }
    } else if !node_has_clearly_exited_origin(
        project,
        composition_id,
        primary_origin.container,
        resolved_target.container,
        primary_rect,
        drop_point,
    ) {
        // Header/padding hysteresis retains canonical ownership, but the Node
        // still has an exact final position. Grow the old owner around that
        // position atomically so a later auto-layout pass cannot move it.
        return retained_origin_intents(
            project,
            composition_id,
            gesture,
            rendered_node_rects,
            drop_point,
        );
    }

    // A multi-selected drag is one semantic move. Resolve ownership from the
    // physically grabbed Node, then keep every selected Node's relative
    // layout by applying that same target in one candidate transaction.
    node_ids
        .into_iter()
        .filter_map(|node_id| {
            rendered_node_rects
                .get(&node_id)
                .copied()
                .map(|final_rect| NodeDropIntent {
                    node_id,
                    final_rect,
                    target: resolved_target,
                })
        })
        .collect()
}

fn retained_origin_intents(
    project: &Project,
    composition_id: Uuid,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    drop_point: egui::Pos2,
) -> Vec<NodeDropIntent> {
    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    node_ids
        .into_iter()
        .filter_map(|node_id| {
            let origin = gesture.origins.get(&node_id)?.container;
            let final_rect = rendered_node_rects.get(&node_id).copied()?;
            let target = retained_origin_reparent_target(
                project,
                composition_id,
                origin,
                final_rect,
                drop_point,
            )?;
            (!rect_contains_rect(target.content_rect, final_rect)).then_some(NodeDropIntent {
                node_id,
                final_rect,
                target,
            })
        })
        .collect()
}

fn retained_origin_reparent_target(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let geometry = reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == origin)?;
    let content_rect = geometry.content_rect.unwrap_or(geometry.visible_rect);
    Some(ReparentTargetEvaluation {
        container: origin,
        visible_rect: geometry.visible_rect,
        content_rect,
        depth: geometry.depth,
        stacking_order: geometry.stacking_order,
        overlap_ratio: overlap_ratio(content_rect, node_rect),
        center_inside: content_rect.contains(node_rect.center()),
        pointer_inside: content_rect.contains(drop_point),
        root_fallback: false,
        score: -0.5,
        kind: ReparentTargetKind::RetainedOrigin,
    })
}

fn target_is_deeper_within_origin(
    project: &Project,
    origin: NodeContainer,
    target: NodeContainer,
) -> bool {
    match (origin, target) {
        (NodeContainer::Composition(composition_id), NodeContainer::Track(track_id)) => {
            project.find_composition_for_track(track_id) == Some(composition_id)
        }
        (NodeContainer::Composition(composition_id), NodeContainer::Clip(clip_id)) => {
            project
                .find_track_for_clip(clip_id)
                .and_then(|track_id| project.find_composition_for_track(track_id))
                == Some(composition_id)
        }
        (NodeContainer::Track(track_id), NodeContainer::Clip(clip_id)) => {
            project.find_track_for_clip(clip_id) == Some(track_id)
        }
        _ => false,
    }
}

fn target_is_authoritative_sibling(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    target: NodeContainer,
    drop_point: egui::Pos2,
) -> bool {
    let origin_parent = parent_container_owner(project, port_owner_for_node_container(origin));
    let target_parent = parent_container_owner(project, port_owner_for_node_container(target));
    if origin == target || origin_parent.is_none() || origin_parent != target_parent {
        return false;
    }
    reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == target)
        .is_some_and(|geometry| {
            geometry
                .content_rect
                .unwrap_or(geometry.visible_rect)
                .contains(drop_point)
        })
}

fn node_has_clearly_exited_origin(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    target: NodeContainer,
    final_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> bool {
    if target_is_deeper_within_origin(project, origin, target)
        || target_is_authoritative_sibling(project, composition_id, origin, target, drop_point)
    {
        return true;
    }
    let origin_geometry = reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == origin);
    origin_geometry.is_some_and(|geometry| {
        !geometry.visible_rect.intersects(final_rect) && !geometry.visible_rect.contains(drop_point)
    })
}

fn record_node_reparent_origins(
    project: &Project,
    layout_edits: &[LayoutEdit],
    state: &mut NodeEditorState,
    gesture_allowed: bool,
) {
    if !gesture_allowed {
        return;
    }
    for edit in layout_edits {
        let LayoutEdit::MoveNode { node_id, .. } = edit else {
            continue;
        };
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        let Some(container) = project.find_node_container(*node_id) else {
            continue;
        };
        state.moved_node_ids.insert(*node_id);
        let gesture = state
            .node_reparent
            .get_or_insert_with(|| NodeEditorReparentGesture {
                origins: HashMap::new(),
                primary_node_id: None,
                hovered_target: None,
                hovered_node_id: None,
                hovered_score: None,
            });
        gesture
            .origins
            .entry(*node_id)
            .or_insert(NodeEditorNodeDragOrigin {
                container,
                position: node.ui_position,
            });
    }
}

fn final_node_positions(
    project: &Project,
    gesture: &NodeEditorReparentGesture,
    layout_edits: &[LayoutEdit],
) -> HashMap<Uuid, [f32; 2]> {
    let mut positions = gesture
        .origins
        .keys()
        .filter_map(|node_id| {
            project
                .get_node(*node_id)
                .map(|node| (*node_id, node.ui_position))
        })
        .collect::<HashMap<_, _>>();
    for edit in layout_edits {
        if let LayoutEdit::MoveNode { node_id, position } = edit {
            if positions.contains_key(node_id) {
                positions.insert(*node_id, *position);
            }
        }
    }
    positions
}

fn primary_node_drop_intent(
    intents: &[NodeDropIntent],
    drop_point: egui::Pos2,
) -> Option<NodeDropIntent> {
    intents
        .iter()
        .copied()
        .find(|intent| intent.final_rect.contains(drop_point))
        .or_else(|| intents.first().copied())
}

fn captured_snarl_drag_node(
    context: &egui::Context,
    snarl: &Snarl<GraphItem>,
    snarl_id: egui::Id,
) -> Option<Uuid> {
    let dragged_id = context.dragged_id()?;
    snarl.node_ids().find_map(|(node_id, item)| {
        if snarl_id.with(("snarl-node", node_id)).with("frame") != dragged_id {
            return None;
        }
        match item {
            GraphItem::Node(project_node_id) => Some(*project_node_id),
            GraphItem::Container(_) | GraphItem::PortAnchor { .. } => None,
        }
    })
}

fn reparent_ineligible_reason(geometry: ReparentContainerGeometry) -> Option<&'static str> {
    geometry.collapsed.then_some("collapsed_hidden_content")
}

fn reported_reparent_target(
    evaluation: Option<ReparentTargetEvaluation>,
    selected: bool,
    active: NodeDropIntent,
) -> Option<ReparentTargetEvaluation> {
    if selected {
        Some(active.target)
    } else {
        evaluation
    }
}

fn register_reparent_drop_targets(
    project: &Project,
    composition_id: Uuid,
    active: NodeDropIntent,
    drop_point: egui::Pos2,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    painter: &egui::Painter,
) {
    for geometry in reparent_container_geometries(project, composition_id) {
        let evaluation = evaluate_reparent_target(geometry, active.final_rect, drop_point);
        let selected = active.target.container == geometry.container;
        let reported = reported_reparent_target(evaluation, selected, active);
        let graph_rect = geometry.content_rect.unwrap_or(geometry.visible_rect);
        let unclipped_rect = to_global * graph_rect;
        let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
        let owner = qa_container_key(port_owner_for_node_container(geometry.container));
        let (overlap, center_inside, pointer_inside, score) =
            reported.map_or((0.0, false, false, None), |evaluation| {
                (
                    evaluation.overlap_ratio,
                    evaluation.center_inside,
                    evaluation.pointer_inside,
                    Some(evaluation.score),
                )
            });
        crate::qa::register_component_with_metadata(
            format!("node_editor.reparent_target.{owner}"),
            "node_reparent_target",
            rect,
            reported.is_some(),
            Some(serde_json::json!({
                "owner": owner,
                "active_node_id": active.node_id,
                "selected": selected,
                "eligible": reported.is_some(),
                "geometry_eligible": evaluation.is_some(),
                "collapsed": geometry.collapsed,
                "ineligible_reason": reparent_ineligible_reason(geometry),
                "selection_kind": selected.then_some(active.target.kind.as_str()),
                "depth": geometry.depth,
                "stacking_order": geometry.stacking_order,
                "overlap_ratio": overlap,
                "center_inside": center_inside,
                "pointer_inside": pointer_inside,
                "root_fallback": selected && active.target.root_fallback,
                "score": score,
                "drop_point": {"x": drop_point.x, "y": drop_point.y},
                "node_rect": qa_rect_metadata(active.final_rect),
                "content_rect": geometry.content_rect.map(qa_rect_metadata),
                "drop_target_rect": qa_rect_metadata(graph_rect),
                "unclipped_rect": qa_rect_metadata(unclipped_rect),
                "visible_in_canvas": rect.is_positive(),
            })),
        );
    }

    let highlight = if active.target.root_fallback {
        active.target.visible_rect
    } else {
        active.target.content_rect
    };
    painter.rect_filled(
        highlight,
        egui::CornerRadius::same(8),
        Color32::from_rgba_premultiplied(78, 190, 128, 22),
    );
    painter.rect_stroke(
        highlight,
        egui::CornerRadius::same(8),
        egui::Stroke::new(
            screen_stroke_in_graph_units(2.0, to_global.scaling),
            Color32::from_rgb(94, 221, 151),
        ),
        egui::StrokeKind::Inside,
    );
}

fn reparent_nodes_from_intents(project: &mut Project, intents: &[NodeDropIntent]) -> bool {
    let mut candidate = project.clone();
    let mut changed = false;
    for intent in intents {
        let node_id = intent.node_id;
        let destination = intent.target.container;
        let containment_rect = estimated_node_rect(&candidate, node_id)
            .map_or(intent.final_rect, |estimated| {
                estimated.union(intent.final_rect)
            });
        if candidate.find_node_container(node_id) == Some(destination) {
            changed |=
                ensure_reparent_hierarchy_contains(&mut candidate, destination, containment_rect);
            continue;
        }
        match candidate.attach_node_to_container(destination, node_id) {
            Ok(()) => {
                ensure_reparent_hierarchy_contains(&mut candidate, destination, containment_rect);
                changed = true;
            }
            Err(error) => {
                log::warn!("Cannot move Node {node_id} to {destination:?}: {error}");
                return false;
            }
        }
    }
    if changed {
        *project = candidate;
    }
    changed
}

fn finish_node_reparent(
    project: &mut Project,
    intents: &[NodeDropIntent],
    gesture: Option<&NodeEditorReparentGesture>,
) -> ReparentReleaseOutcome {
    if intents.is_empty() {
        return ReparentReleaseOutcome::NoIntent;
    }
    let mut baseline = project.clone();
    restore_reparent_origins(&mut baseline, gesture);
    let mut candidate = project.clone();
    if reparent_nodes_from_intents(&mut candidate, intents) {
        let composition_ids = intents
            .iter()
            .filter_map(|intent| {
                port_owner_composition(
                    &candidate,
                    port_owner_for_node_container(intent.target.container),
                )
            })
            .collect::<HashSet<_>>();
        let introduces_invalid_layout = composition_ids.into_iter().any(|composition_id| {
            !container_hierarchy_needs_reflow(&baseline, composition_id)
                && container_hierarchy_needs_reflow(&candidate, composition_id)
        });
        if !introduces_invalid_layout {
            *project = candidate;
            return ReparentReleaseOutcome::Applied;
        }
        log::warn!("Rejected Node reparent because it would require automatic layout repair");
    }

    // The live graph follows the pointer during the drag. If the semantic
    // candidate is rejected, roll every participating Node back together so
    // a half-applied layout cannot remain under the old owner or splice a wire.
    restore_reparent_origins(project, gesture);
    ReparentReleaseOutcome::Rejected
}

fn restore_reparent_origins(project: &mut Project, gesture: Option<&NodeEditorReparentGesture>) {
    if let Some(gesture) = gesture {
        for (node_id, origin) in &gesture.origins {
            if let Some(node) = project.get_node_mut(*node_id) {
                node.ui_position = origin.position;
            }
        }
    }
}

fn splice_node_for_release(
    outcome: ReparentReleaseOutcome,
    moved_node_ids: &HashSet<Uuid>,
    captured_primary_node_id: Option<Uuid>,
) -> Option<Uuid> {
    if outcome != ReparentReleaseOutcome::NoIntent || moved_node_ids.len() != 1 {
        return None;
    }
    let node_id = moved_node_ids.iter().next().copied()?;
    (captured_primary_node_id == Some(node_id)).then_some(node_id)
}

#[cfg(test)]
fn reparent_test_node_at_drop(
    project: &mut Project,
    composition_id: Uuid,
    node_id: Uuid,
    drop_point: egui::Pos2,
) -> bool {
    let node_size = estimated_node_size(project, node_id);
    let final_rect = egui::Rect::from_center_size(drop_point, node_size);
    let Some(node) = project.get_node_mut(node_id) else {
        return false;
    };
    node.ui_position = [final_rect.min.x, final_rect.min.y];
    let Some(target) =
        deepest_legal_reparent_target(project, composition_id, final_rect, drop_point)
    else {
        return false;
    };
    reparent_nodes_from_intents(
        project,
        &[NodeDropIntent {
            node_id,
            final_rect,
            target,
        }],
    )
}

fn container_rect(position: [f32; 2], size: [f32; 2]) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(position[0], position[1]),
        egui::vec2(
            size[0].max(MIN_CONTAINER_SIZE.x),
            size[1].max(MIN_CONTAINER_SIZE.y),
        ),
    )
}

fn ensure_container_hierarchy_contains(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    let owner = port_owner_for_node_container(container);
    let mut changed = grow_container_to_rect(project, owner, node_rect);

    // Creation and auto-layout clamp their leaf Node to the owning container's
    // minimum content edge. Preserve existing container origins here and grow
    // only the far edges, otherwise adding an item can move an entire existing
    // hierarchy merely because legacy child chrome sits inside its margin.
    let parent_track = match container {
        NodeContainer::Clip(clip_id) => project.find_track_for_clip(clip_id),
        NodeContainer::Track(track_id) => Some(track_id),
        NodeContainer::Composition(_) => None,
    };
    if let Some(track_id) = parent_track {
        if let NodeContainer::Clip(clip_id) = container {
            if let Some(clip) = project.get_clip(clip_id) {
                let clip_rect = container_rect(clip.ui_position, clip.ui_size);
                changed |= grow_container_to_rect(project, PortOwner::Track(track_id), clip_rect);
            }
        }
        if let Some(composition_id) = project.find_composition_for_track(track_id) {
            if let Some(track) = project.get_track(track_id) {
                let track_rect = container_rect(track.ui_position, track.ui_size);
                changed |= grow_container_to_rect(
                    project,
                    PortOwner::Composition(composition_id),
                    track_rect,
                );
            }
        }
    }
    changed
}

fn ensure_reparent_hierarchy_contains(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    let owner = port_owner_for_node_container(container);
    let mut changed = grow_container_to_rect_all_edges(project, owner, node_rect);

    // Propagate each *updated child container rectangle*, not only the Node.
    // Expanding the min edge intentionally changes only container chrome;
    // child Nodes keep their absolute graph coordinates and therefore their
    // exact drop position.
    let mut child_owner = owner;
    while let Some(parent_owner) = parent_container_owner(project, child_owner) {
        let Some(child) = container_visual(project, child_owner) else {
            break;
        };
        let child_rect = container_rect(child.position, child.size);
        changed |= grow_container_to_rect_all_edges(project, parent_owner, child_rect);
        child_owner = parent_owner;
    }
    changed
}

fn port_owner_for_node_container(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

fn port_owner_composition(project: &Project, owner: PortOwner) -> Option<Uuid> {
    match owner {
        PortOwner::Composition(composition_id) => project
            .get_composition(composition_id)
            .map(|_| composition_id),
        PortOwner::Track(track_id) => project.find_composition_for_track(track_id),
        PortOwner::Clip(clip_id) => project
            .find_track_for_clip(clip_id)
            .and_then(|track_id| project.find_composition_for_track(track_id)),
        PortOwner::Node(node_id) => project.find_node_container(node_id).and_then(|container| {
            port_owner_composition(project, port_owner_for_node_container(container))
        }),
    }
}

fn parent_container_owner(project: &Project, owner: PortOwner) -> Option<PortOwner> {
    match owner {
        PortOwner::Composition(_) | PortOwner::Node(_) => None,
        PortOwner::Track(track_id) => project
            .find_composition_for_track(track_id)
            .map(PortOwner::Composition),
        PortOwner::Clip(clip_id) => project.find_track_for_clip(clip_id).map(PortOwner::Track),
    }
}

fn grow_container_to_rect(project: &mut Project, owner: PortOwner, rect: egui::Rect) -> bool {
    let Some(visual) = container_visual(project, owner) else {
        return false;
    };
    let (right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) | PortOwner::Clip(_) => {
            (AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_BOTTOM)
        }
        PortOwner::Node(_) => return false,
    };
    let size = [
        visual.size[0].max(rect.right() - visual.position[0] + right_margin),
        visual.size[1].max(rect.bottom() - visual.position[1] + bottom_margin),
    ];
    set_container_size(project, owner, size)
}

fn grow_container_to_rect_all_edges(
    project: &mut Project,
    owner: PortOwner,
    rect: egui::Rect,
) -> bool {
    let Some(visual) = container_visual(project, owner) else {
        return false;
    };
    let (left_margin, top_margin, right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_LEFT,
            AUTO_LAYOUT_COMPOSITION_TOP,
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) => (
            AUTO_LAYOUT_TRACK_LEFT,
            AUTO_LAYOUT_TRACK_TOP,
            AUTO_LAYOUT_TRACK_RIGHT,
            AUTO_LAYOUT_TRACK_BOTTOM,
        ),
        PortOwner::Clip(_) => (
            AUTO_LAYOUT_TRACK_LEFT,
            AUTO_LAYOUT_CLIP_TOP,
            AUTO_LAYOUT_TRACK_RIGHT,
            AUTO_LAYOUT_TRACK_BOTTOM,
        ),
        PortOwner::Node(_) => return false,
    };
    let old_rect = container_rect(visual.position, visual.size);
    let left = old_rect.left().min(rect.left() - left_margin);
    let top = old_rect.top().min(rect.top() - top_margin);
    let right = old_rect.right().max(rect.right() + right_margin);
    let bottom = old_rect.bottom().max(rect.bottom() + bottom_margin);
    let position = [left, top];
    let size = [
        (right - left).max(MIN_CONTAINER_SIZE.x),
        (bottom - top).max(MIN_CONTAINER_SIZE.y),
    ];
    set_container_geometry(project, owner, position, size)
}

fn create_clip_at_free_slot(
    project: &mut Project,
    desired: egui::Pos2,
    composition_id: Uuid,
    name: &str,
) -> Option<Uuid> {
    let composition = project.get_composition(composition_id)?;
    let track_id = composition
        .track_ids
        .iter()
        .rev()
        .find(|track_id| {
            project.get_track(**track_id).is_some_and(|track| {
                container_rect(track.ui_position, track.ui_size).contains(desired)
            })
        })
        .copied()
        .or_else(|| composition.track_ids.first().copied())?;
    let track = project.get_track(track_id)?.clone();
    let mut clip = library::model::Clip::new(name, 0.0, 5.0);
    let size = egui::vec2(clip.ui_size[0], clip.ui_size[1]);
    let min = egui::pos2(
        track.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        track.ui_position[1] + AUTO_LAYOUT_TRACK_TOP,
    );
    let mut candidate =
        egui::Rect::from_min_size(egui::pos2(desired.x.max(min.x), desired.y.max(min.y)), size);
    for existing_id in &track.clip_ids {
        let Some(existing) = project.get_clip(*existing_id) else {
            continue;
        };
        let existing_rect = container_rect(existing.ui_position, existing.ui_size);
        if candidate.intersects(existing_rect) {
            candidate = candidate.translate(egui::vec2(
                0.0,
                existing_rect.bottom() - candidate.top() + AUTO_LAYOUT_ROW_GAP,
            ));
        }
    }
    clip.ui_position = [candidate.min.x, candidate.min.y];
    let clip_id = clip.id;
    project.add_clip(clip);
    if let Err(error) = project.attach_clip_to_track(track_id, clip_id) {
        project.remove_clip(clip_id);
        log::warn!("Cannot add Clip to Track: {error}");
        return None;
    }
    ensure_container_hierarchy_contains(project, NodeContainer::Track(track_id), candidate);
    Some(clip_id)
}

fn create_track_at_free_slot(
    project: &mut Project,
    desired: egui::Pos2,
    composition_id: Uuid,
    name: &str,
) -> Option<Uuid> {
    let composition = project.get_composition(composition_id)?.clone();
    let mut track = library::model::Track::new(name);
    let size = egui::vec2(track.ui_size[0], track.ui_size[1]);
    let min = egui::pos2(
        composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_LEFT,
        composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP,
    );
    let mut candidate =
        egui::Rect::from_min_size(egui::pos2(desired.x.max(min.x), desired.y.max(min.y)), size);
    let occupied = composition
        .track_ids
        .iter()
        .filter_map(|track_id| project.get_track(*track_id))
        .map(|track| {
            egui::Rect::from_min_size(
                egui::pos2(track.ui_position[0], track.ui_position[1]),
                egui::vec2(track.ui_size[0], track.ui_size[1]),
            )
        })
        .collect::<Vec<_>>();
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| candidate.expand(8.0).intersects(**other))
            .map(|other| other.bottom() + AUTO_LAYOUT_TRACK_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(candidate.min.x, next_y), size);
    }
    track.ui_position = [candidate.min.x, candidate.min.y];
    let track_id = track.id;
    project.add_track(track);
    if let Err(error) = project.attach_track_to_composition(composition_id, track_id) {
        project.remove_track(track_id);
        log::warn!("Cannot add track to composition: {error}");
        return None;
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size[0] = composition.ui_size[0]
            .max(candidate.right() - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT);
        composition.ui_size[1] = composition.ui_size[1]
            .max(candidate.bottom() - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM);
    }
    Some(track_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::generator_node;
    use library::animation::EasingFunction;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::frame::draw_type::DrawStyle;
    use library::model::frame::entity::StyleConfig;
    use library::model::project::{
        ProjectConnection, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
        SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT, VALUE_INPUT_PORT,
    };
    use library::model::property::{Keyframe, Property, PropertyMap};
    use library::model::Composition;
    use library::plugin::{
        FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, Plugin, StylePlugin,
    };

    #[test]
    fn typed_port_owner_composition_ignores_same_uuid_clip() {
        let shared_id = Uuid::new_v4();
        let mut project = Project::new("typed owner composition");

        let (clip_composition, clip_track) =
            Composition::new("Clip composition", 1920, 1080, 30.0, 10.0);
        let clip_composition_id = clip_composition.id;
        let clip_track_id = clip_track.id;
        let mut collision = Clip::new("same UUID Clip", 0.0, 5.0);
        collision.id = shared_id;
        project.add_track(clip_track);
        project.add_composition(clip_composition);
        project.add_clip(collision);
        project
            .attach_clip_to_track(clip_track_id, shared_id)
            .unwrap();

        let (node_composition, node_track) =
            Composition::new("Node composition", 1920, 1080, 30.0, 10.0);
        let node_composition_id = node_composition.id;
        let mut node = Node::new_merge("same UUID Node");
        node.id = shared_id;
        project.add_track(node_track);
        project.add_composition(node_composition);
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Composition(node_composition_id), shared_id)
            .unwrap();

        assert_eq!(
            project.find_containing_composition(shared_id),
            Some(clip_composition_id)
        );
        assert_eq!(
            port_owner_composition(&project, PortOwner::Node(shared_id)),
            Some(node_composition_id)
        );
        assert_eq!(
            port_owner_composition(&project, PortOwner::Clip(shared_id)),
            Some(clip_composition_id)
        );
    }

    struct RuntimeCatalogStylePlugin {
        id: String,
    }

    impl Plugin for RuntimeCatalogStylePlugin {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> String {
            "Runtime Hatch".to_string()
        }

        fn category(&self) -> String {
            "Runtime QA".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl StylePlugin for RuntimeCatalogStylePlugin {
        fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
            OperationDescriptor::style(self.id.clone(), self.name(), Vec::new())
        }

        fn evaluate_source(
            &self,
            _context: &FrameEvaluationContext,
            source_id: Uuid,
            _properties: &PropertyMap,
            _eval_time: f64,
        ) -> Option<StyleConfig> {
            Some(StyleConfig {
                id: source_id,
                style: DrawStyle::Fill {
                    color: Color::white(),
                    offset: 0.0,
                },
            })
        }
    }

    #[test]
    fn time_modulo_has_explicit_value_node_presentation() {
        let mut project = Project::new("value node presentation");
        let node = Node::new_time_modulo("Time Modulo");
        let node_id = node.id;
        project.add_node(node);

        assert_eq!(VALUE_NODE_CATEGORY_LABEL, "Value");
        assert_eq!(
            value_operation_label(library::model::ValueContent::TimeModulo),
            "Time Modulo"
        );
        assert_eq!(node_icon(&project, node_id), "%");
        let palette = node_palette(&project, node_id);
        assert_eq!(palette.body, Color32::from_rgb(28, 41, 46));
        assert_eq!(palette.header, Color32::from_rgb(39, 83, 95));
        assert_eq!(palette.accent, Color32::from_rgb(91, 197, 218));
        assert_eq!(estimated_node_size(&project, node_id).y, 220.0);
    }

    #[test]
    fn time_modulo_add_item_creates_a_native_node_with_explicit_time_input_wiring() {
        let plugins = PluginManager::default();
        let items = node_create_menu_items(&plugins);
        let item = items
            .iter()
            .find(|item| item.value == NodeCreateRequest::TimeModulo)
            .expect("Time Modulo is exposed by the shared Add catalog");
        assert_eq!(item.label, "Time Modulo");
        assert_eq!(item.category.as_deref(), Some("Timing / Values"));
        assert_eq!(
            item.qa_id.as_deref(),
            Some("node_editor.menu.create.time_modulo")
        );
        let matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
            &items,
            "loop value",
        );
        assert!(matches.iter().any(|index| items[*index] == *item));

        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let node = create_operation_node_for_request(&item.value, &plugins)
            .expect("native value request creates a Node");
        let node_id = node.id;
        let clip = project.get_clip(clip_id).unwrap();
        let position = egui::pos2(clip.ui_position[0] + 260.0, clip.ui_position[1] + 280.0);
        assert!(create_prebuilt_node(
            &mut project,
            position,
            node,
            composition_id,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(node_id), VALUE_INPUT_PORT),
            )
            .expect("container Time connects explicitly to the value input");
        let connection = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap();
        assert_eq!(connection.from.port, TIME_PORT);
        assert_eq!(connection.to.port, VALUE_INPUT_PORT);
    }

    #[test]
    fn real_snarl_connected_output_fans_out_to_time_value_without_reconnect_or_pan() {
        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let mut modulo = Node::new_time_modulo("Time Modulo");
        modulo.ui_position = [520.0, 620.0];
        let modulo_id = modulo.id;
        project.add_node(modulo);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), modulo_id)
            .unwrap();
        let original_time_connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT)
                    && connection.to.port == TIME_PORT
            })
            .expect("fixture Time output is already connected")
            .clone();
        let initial = project.clone();
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut queued = Vec::new();
        let mut gesture_transforms = Vec::new();
        reset_test_rects();

        let source_address = PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT);
        let target_address = PortAddress::new(PortOwner::Node(modulo_id), VALUE_INPUT_PORT);
        let source_key = RenderedPortKey {
            address: source_address.clone(),
            direction: PortDirection::Output,
        };
        let target_key = RenderedPortKey {
            address: target_address.clone(),
            direction: PortDirection::Input,
        };

        // Let Snarl finish its initial look-at/layout pass before using the
        // published socket rectangles as physical input coordinates.
        for frame in 0..6 {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        rendered_ports.lock().unwrap().clear();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::default();
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut queued,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: state.wire_gesture.is_some()
                                || state.normal_connect_gesture.is_some()
                                || state.normal_connect_cancel_pending_release,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::clone(&rendered_ports),
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("time-value-port-drag", composition_id)),
                            ui,
                        );
                        drop(viewer);
                        let edges =
                            register_rendered_edges(&project, &rendered_ports, canvas_clip, None);
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: &edges,
                                rendered_ports: &rendered_ports,
                                canvas_clip,
                                graph_item_rects: &exclusions,
                                to_global,
                            },
                        ));
                    });
                },
            ));
        }
        assert!(queued.is_empty());
        let ports = rendered_ports.lock().unwrap();
        let source_rect = *ports.get(&source_key).expect("actual Time output socket");
        let target_rect = *ports.get(&target_key).expect("actual value input socket");
        drop(ports);
        assert!(source_rect.is_positive());
        assert!(target_rect.is_positive());
        assert_eq!(
            test_rect(&qa_port_id(
                &project,
                Some(GraphItem::PortAnchor {
                    owner: PortOwner::Clip(clip_id),
                    kind: PortAnchorKind::InternalMetadata,
                }),
                "output",
                TIME_PORT,
            ))
            .expect("Time output QA hit")
            .center(),
            source_rect.center(),
        );
        assert_eq!(
            test_rect(&qa_port_id(
                &project,
                Some(GraphItem::Node(modulo_id)),
                "input",
                VALUE_INPUT_PORT,
            ))
            .expect("value input QA hit")
            .center(),
            target_rect.center(),
        );

        let source = source_rect.center();
        let target = target_rect.center();
        let drag_start = source + egui::vec2(WIRE_DRAG_THRESHOLD + 2.0, 0.0);
        let input_frames = vec![
            // First cancel the forward fan-out. Snarl still owns internal
            // drag state, so suppression must survive through release.
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(target)],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            // Then perform the same connected-output fan-out successfully.
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            // Cross egui's drag threshold while the captured Time output is
            // still nearby. A single endpoint-to-endpoint jump can enter the
            // destination before Snarl has observed `drag_started_by`.
            vec![egui::Event::PointerMoved(drag_start)],
            vec![egui::Event::PointerMoved(target)],
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for (frame, events) in input_frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((frame + 6) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        rendered_ports.lock().unwrap().clear();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::default();
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut queued,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: state.wire_gesture.is_some()
                                || state.normal_connect_gesture.is_some()
                                || state.normal_connect_cancel_pending_release,
                            locked_canvas_transform: state
                                .normal_connect_gesture
                                .as_ref()
                                .map(|gesture| gesture.canvas_transform),
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::clone(&rendered_ports),
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("time-value-port-drag", composition_id)),
                            ui,
                        );
                        drop(viewer);
                        let edges =
                            register_rendered_edges(&project, &rendered_ports, canvas_clip, None);
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: &edges,
                                rendered_ports: &rendered_ports,
                                canvas_clip,
                                graph_item_rects: &exclusions,
                                to_global,
                            },
                        ));
                        gesture_transforms.push(to_global);
                    });
                },
            ));
            match frame {
                1 | 2 | 6..=8 => {
                    assert!(state.normal_connect_gesture.is_some());
                    assert!(!state.normal_connect_cancel_pending_release);
                }
                3 => {
                    assert!(state.normal_connect_gesture.is_none());
                    assert!(state.normal_connect_cancel_pending_release);
                    assert!(queued.is_empty());
                }
                4 | 5 => {
                    assert!(state.normal_connect_gesture.is_none());
                    assert!(!state.normal_connect_cancel_pending_release);
                    assert!(queued.is_empty());
                }
                0 | 9 => {
                    assert!(state.normal_connect_gesture.is_none());
                    assert!(!state.normal_connect_cancel_pending_release);
                }
                unexpected => panic!("unexpected input frame {unexpected}"),
            }
            assert!(!state.normal_wire_drag_active);
        }

        assert!(gesture_transforms.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(state.wire_gesture.is_none());
        assert!(state.selected_connection_id.is_none());
        assert!(
            queued.iter().any(|queued| {
                matches!(
                    queued,
                    QueuedNodeEdit::Atomic(NodeEdit::Connect { from, to })
                        if *from == source_address && *to == target_address
                )
            }),
            "real Snarl drag queued {queued:?}; wire state: {:?}",
            state.wire_gesture,
        );
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            queued,
            &mut history,
            &mut state,
        ));
        assert!(project.connections.iter().any(|connection| {
            connection.from == source_address && connection.to == target_address
        }));
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == original_time_connection.id),
            Some(&original_time_connection),
            "fan-out must preserve the original Time wire byte-for-byte"
        );
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    fn fixture() -> (Project, Uuid, Uuid, Uuid, Uuid, Uuid) {
        let mut project = Project::new("Node editor test");
        let (mut composition, mut track) =
            library::model::Composition::new("Main", 1920, 1080, 30.0, 10.0);
        composition.ui_position = [10.0, 20.0];
        composition.ui_size = [1400.0, 1000.0];
        track.ui_position = [110.0, 140.0];
        track.ui_size = [1100.0, 720.0];
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);

        let mut clip = library::model::Clip::new("Clip", 1.0, 5.0);
        clip.ui_position = [260.0, 260.0];
        clip.ui_size = [760.0, 480.0];
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let mut solid = generator_node(
            "Solid",
            GeneratorNodeRequest::Solid {
                color: library::model::frame::color::Color {
                    r: 10,
                    g: 20,
                    b: 30,
                    a: 255,
                },
            },
        );
        solid.ui_position = [450.0, 390.0];
        let solid_id = solid.id;
        project.add_node(solid);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), solid_id)
            .unwrap();

        let mut merge = Node::new_merge("Merge");
        merge.ui_position = [770.0, 390.0];
        let merge_id = merge.id;
        project.add_node(merge);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), merge_id)
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(solid_id), TIME_PORT),
            )
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(solid_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .unwrap();

        (
            project,
            composition_id,
            track_id,
            clip_id,
            solid_id,
            merge_id,
        )
    }

    fn style_graph_factory() -> library::editor::project_service::ProjectManager {
        library::editor::project_service::ProjectManager::new(
            Arc::new(RwLock::new(Project::new("detached graph factory"))),
            Arc::new(PluginManager::default()),
        )
    }

    fn run_wire_interaction_frames(
        project: &Project,
        edge: &RenderedEdge,
        rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
        state: &mut NodeEditorState,
        frames: Vec<Vec<egui::Event>>,
    ) -> Vec<QueuedNodeEdit> {
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let mut queued = Vec::new();
        for (frame, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
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

    fn plugin_operation_component(node: &Node) -> Option<&str> {
        match node.content() {
            NodeContent::PluginOperation(operation) => Some(&operation.component_id),
            _ => None,
        }
    }

    fn assert_detached_graph_has_clean_ltr_layout(project: &Project, graph: &NodeGraphBundle) {
        let mut measurement_project = project.clone();
        for node in &graph.nodes {
            measurement_project.add_node(node.clone());
        }
        let rects = graph
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id,
                    egui::Rect::from_min_size(
                        egui::pos2(node.ui_position[0], node.ui_position[1]),
                        estimated_node_size(&measurement_project, node.id),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();

        for (index, left) in graph.nodes.iter().enumerate() {
            for right in &graph.nodes[index + 1..] {
                assert!(
                    !padded_intersection(rects[&left.id], rects[&right.id]),
                    "detached Nodes overlap or have less than {AUTO_LAYOUT_NODE_PADDING}px gap: {} {:?} / {} {:?}",
                    left.id,
                    rects[&left.id],
                    right.id,
                    rects[&right.id],
                );
            }
        }

        let ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let edges = graph
            .connections
            .iter()
            .filter_map(|connection| {
                let (PortOwner::Node(from), PortOwner::Node(to)) =
                    (connection.from.owner, connection.to.owner)
                else {
                    return None;
                };
                Some((from, to))
            })
            .collect::<Vec<_>>();
        let ranks = rank_nodes_by_scc(&ids, &edges);
        for (from, to) in edges {
            if ranks[&from] == ranks[&to] {
                continue;
            }
            assert!(
                rects[&from].right() + AUTO_LAYOUT_NODE_PADDING <= rects[&to].left(),
                "detached edge is not left-to-right: {from} {:?} -> {to} {:?}",
                rects[&from],
                rects[&to],
            );
        }
    }

    #[test]
    fn add_menu_discovers_descriptor_operations_and_wire_menu_filters_them_by_type() {
        let plugins = Arc::new(PluginManager::default());
        let runtime_style_id = "runtime.qa.style.hatch";
        plugins.register_style_plugin(Arc::new(RuntimeCatalogStylePlugin {
            id: runtime_style_id.to_string(),
        }));
        let items = node_create_menu_items(plugins.as_ref());
        assert!(items.iter().all(|item| item.category.is_some()));
        assert!(items
            .iter()
            .any(|item| matches!(item.value, NodeCreateRequest::SkSL)));

        for component_id in ["fill", "stroke"] {
            let style = items
                .iter()
                .find(|item| {
                    matches!(&item.value, NodeCreateRequest::Style(id) if id == component_id)
                })
                .unwrap();
            assert_eq!(
                style.qa_metadata.as_ref().unwrap()["component_id"],
                component_id
            );
            assert_eq!(
                style.qa_metadata.as_ref().unwrap()["operation"],
                STYLE_APPLY_OPERATION
            );
        }
        let backplate = items
            .iter()
            .find(
                |item| matches!(&item.value, NodeCreateRequest::Decorator(id) if id == "backplate"),
            )
            .unwrap();
        assert_eq!(
            backplate.qa_id.as_deref(),
            Some("node_editor.menu.create.decorator:backplate")
        );

        let runtime_style = items
            .iter()
            .find(|item| {
                matches!(&item.value, NodeCreateRequest::Style(id) if id == runtime_style_id)
            })
            .expect("a style registered after PluginManager construction must be discoverable");
        assert_eq!(
            runtime_style.qa_id.as_deref(),
            Some("node_editor.menu.create.style:runtime.qa.style.hatch")
        );
        assert_eq!(
            runtime_style.qa_metadata.as_ref().unwrap()["label"],
            "Runtime Hatch"
        );
        assert!(runtime_style
            .keywords
            .iter()
            .any(|keyword| keyword == runtime_style_id));

        let blur = items
            .iter()
            .find(|item| {
                matches!(&item.value, NodeCreateRequest::Effect(effect_id) if effect_id == "blur")
            })
            .expect("built-in Blur effect is exposed in the Add menu");
        assert!(blur
            .category
            .as_deref()
            .is_some_and(|category| category.starts_with("Image Effects /")));
        assert_eq!(
            blur.qa_id.as_deref(),
            Some("node_editor.menu.create.effect:blur")
        );
        assert!(blur.keywords.iter().any(|keyword| keyword == "blur"));
        let matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
            &items,
            "runtime.qa hatch",
        );
        assert!(matches.iter().any(|index| items[*index] == *runtime_style));

        let (mut project, _, _, clip_id, _, _) = fixture();
        let shape = generator_node(
            "Shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        );
        let shape_id = shape.id;
        let transform = plugins.create_effector_operation_node("transform").unwrap();
        let transform_id = transform.id;
        project.add_node(shape);
        project.add_node(transform);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), shape_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), transform_id)
            .unwrap();
        let shape_connection = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
            )
            .unwrap();
        let splice_items = wire_splice_menu_items(&project, shape_connection, plugins.as_ref());
        assert!(splice_items.iter().any(|item| {
            matches!(&item.value, NodeCreateRequest::Decorator(id) if id == "backplate")
        }));
        assert!(!splice_items.iter().any(|item| {
            matches!(&item.value, NodeCreateRequest::Style(id) if id == runtime_style_id)
        }));
    }

    #[test]
    fn node_enabled_context_command_is_atomic_and_undoable() {
        let (mut project, _, _, _, node_id, _) = fixture();
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        let mut state = NodeEditorState::default();
        assert!(apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
                node_id,
                enabled: false,
            })],
            &mut history,
            &mut state,
        ));
        assert!(!project.get_node(node_id).unwrap().enabled);
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn merge_wire_layer_order_and_authored_blend_are_canonical_and_undoable() {
        let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
        let first_connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .unwrap()
            .id;
        let mut second = generator_node(
            "Second Solid",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        second.ui_position = [450.0, 560.0];
        let second_id = second.id;
        project.add_node(second);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), second_id)
            .unwrap();
        let second_connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .unwrap();

        let first = project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection_id)
            .unwrap();
        let second = project
            .connections
            .iter()
            .find(|connection| connection.id == second_connection_id)
            .unwrap();
        assert!(connection_supports_authored_blend(&project, first));
        assert!(connection_supports_authored_blend(&project, second));
        assert_eq!(
            wire_order_menu_state(&project, first),
            Some(WireOrderMenuState {
                back_to_front_index: 0,
                layer_count: 2,
            })
        );
        assert_eq!(
            wire_order_menu_state(&project, second),
            Some(WireOrderMenuState {
                back_to_front_index: 1,
                layer_count: 2,
            })
        );

        // Disabled boundary actions are true no-ops: no Project change and no
        // extra history snapshot even if the QA bridge injects their click.
        let boundary_initial = project.clone();
        let mut boundary_history = HistoryManager::new();
        boundary_history.push_project_state(boundary_initial.clone());
        let mut state = NodeEditorState::default();
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: first_connection_id,
                new_order: 0,
            })],
            &mut boundary_history,
            &mut state,
        ));
        assert_eq!(project, boundary_initial);
        assert_eq!(boundary_history.undo_depth(), 1);
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: second_connection_id,
                new_order: 1,
            })],
            &mut boundary_history,
            &mut state,
        ));
        assert_eq!(project, boundary_initial);
        assert_eq!(boundary_history.undo_depth(), 1);

        let blend_initial = project.clone();
        let original_first = project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection_id)
            .unwrap()
            .clone();
        let mut blend_history = HistoryManager::new();
        blend_history.push_project_state(blend_initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                connection_id: first_connection_id,
                blend_mode: BlendMode::Multiply,
            })],
            &mut blend_history,
            &mut state,
        ));
        let blended_first = project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection_id)
            .unwrap();
        assert_eq!(blended_first.id, original_first.id);
        assert_eq!(blended_first.from, original_first.from);
        assert_eq!(blended_first.to, original_first.to);
        assert_eq!(blended_first.order, original_first.order);
        assert_eq!(blended_first.blend_mode, BlendMode::Multiply);
        let blend_edited = project.clone();
        assert_single_gesture_undo_redo(&mut blend_history, &blend_initial, &blend_edited);

        let reorder_initial = project.clone();
        let original_second_blend = project
            .connections
            .iter()
            .find(|connection| connection.id == second_connection_id)
            .unwrap()
            .blend_mode;
        let mut reorder_history = HistoryManager::new();
        reorder_history.push_project_state(reorder_initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: first_connection_id,
                new_order: 1,
            })],
            &mut reorder_history,
            &mut state,
        ));
        let mut merge_connections = project
            .connections
            .iter()
            .filter(|connection| {
                connection.to == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .collect::<Vec<_>>();
        merge_connections.sort_by_key(|connection| connection.order);
        assert_eq!(
            merge_connections
                .iter()
                .map(|connection| (connection.id, connection.order))
                .collect::<Vec<_>>(),
            vec![(second_connection_id, 0), (first_connection_id, 1)]
        );
        assert_eq!(merge_connections[0].blend_mode, original_second_blend);
        assert_eq!(merge_connections[1].blend_mode, BlendMode::Multiply);
        assert_eq!(merge_connections[1].from, original_first.from);
        assert_eq!(merge_connections[1].to, original_first.to);
        let reorder_edited = project.clone();
        assert_single_gesture_undo_redo(&mut reorder_history, &reorder_initial, &reorder_edited);

        let no_op_initial = project.clone();
        let mut no_op_history = HistoryManager::new();
        no_op_history.push_project_state(no_op_initial.clone());
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![
                QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                    connection_id: first_connection_id,
                    new_order: 1,
                }),
                QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                    connection_id: first_connection_id,
                    blend_mode: BlendMode::Multiply,
                }),
            ],
            &mut no_op_history,
            &mut state,
        ));
        assert_eq!(project, no_op_initial);
        assert_eq!(no_op_history.undo_depth(), 1);

        let time_connection = project
            .connections
            .iter()
            .find(|connection| connection.from.port == TIME_PORT)
            .unwrap()
            .clone();
        assert!(!connection_supports_authored_blend(
            &project,
            &time_connection
        ));
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                connection_id: time_connection.id,
                blend_mode: BlendMode::Add,
            })],
            &mut no_op_history,
            &mut state,
        ));
        assert_eq!(project, no_op_initial);
        assert_eq!(no_op_history.undo_depth(), 1);
    }

    #[test]
    fn merge_body_rows_are_back_to_front_and_keep_per_wire_blend_and_source_identity() {
        let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
        let single_layer_estimated = estimated_node_size(&project, merge_id);
        let first_connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .expect("fixture Merge connection")
            .id;
        project
            .set_connection_blend_mode(first_connection_id, BlendMode::Add)
            .expect("first wire Add");

        let mut middle = generator_node(
            "Middle Green",
            GeneratorNodeRequest::Solid {
                color: Color {
                    r: 0,
                    g: 255,
                    b: 0,
                    a: 255,
                },
            },
        );
        middle.ui_position = [490.0, 520.0];
        let middle_id = middle.id;
        project.add_node(middle);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), middle_id)
            .expect("attach middle source");
        let middle_connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(middle_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .expect("connect middle source");
        project
            .set_connection_blend_mode(middle_connection_id, BlendMode::Multiply)
            .expect("middle wire Multiply");

        let mut front = generator_node(
            "Front Blue",
            GeneratorNodeRequest::Solid {
                color: Color {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 255,
                },
            },
        );
        front.ui_position = [530.0, 650.0];
        let front_id = front.id;
        project.add_node(front);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), front_id)
            .expect("attach front source");
        let front_connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(front_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .expect("connect front source");
        project
            .set_connection_blend_mode(front_connection_id, BlendMode::Screen)
            .expect("front wire Screen");

        let rows = merge_layer_rows(&project, merge_id);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.connection_id,
                    row.back_to_front_index,
                    row.authored_order,
                    row.authored_blend_mode,
                    row.source.owner,
                    row.source_label.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    first_connection_id,
                    0,
                    0,
                    BlendMode::Add,
                    PortOwner::Node(solid_id),
                    "Node · Solid",
                ),
                (
                    middle_connection_id,
                    1,
                    1,
                    BlendMode::Multiply,
                    PortOwner::Node(middle_id),
                    "Node · Middle Green",
                ),
                (
                    front_connection_id,
                    2,
                    2,
                    BlendMode::Screen,
                    PortOwner::Node(front_id),
                    "Node · Front Blue",
                ),
            ]
        );
        assert!(rows.iter().all(|row| {
            row.merge_id == merge_id && row.layer_count == 3 && row.authored_blend_available
        }));

        let estimated = estimated_node_size(&project, merge_id);
        assert_eq!(estimated.x, 506.0);
        assert_eq!(estimated.x, estimated_merge_node_width());
        assert_eq!(estimated_node_size(&project, solid_id).x, 462.0);
        assert_eq!(estimated_node_width(), 462.0);
        assert!(estimated.y > single_layer_estimated.y);
        let (rects, _, rendered_transform, _) =
            render_test_graph_with_context_menu_exclusions(&project, composition_id);
        let rendered_merge = rects
            .get(&format!("node_editor.node:{merge_id}"))
            .expect("rendered Merge card");
        assert!(rendered_merge.width() <= estimated.x * rendered_transform.scaling + 1.0);
        assert!(rendered_merge.height() <= estimated.y * rendered_transform.scaling + 1.0);
        let port_rects = [
            qa_port_id(
                &project,
                Some(GraphItem::Node(merge_id)),
                "input",
                MERGE_IMAGES_PORT,
            ),
            qa_port_id(
                &project,
                Some(GraphItem::Node(merge_id)),
                "output",
                IMAGE_OUTPUT_PORT,
            ),
        ]
        .map(|component_id| {
            *rects
                .get(&component_id)
                .unwrap_or_else(|| panic!("missing Merge port {component_id}"))
        });
        for row in &rows {
            for component_id in [
                format!("node_editor.merge_layer:{merge_id}:{}", row.connection_id),
                format!(
                    "node_editor.merge_layer.blend_select:{merge_id}:{}",
                    row.connection_id
                ),
                format!(
                    "node_editor.merge_layer.order_back:{merge_id}:{}",
                    row.connection_id
                ),
                format!(
                    "node_editor.merge_layer.order_front:{merge_id}:{}",
                    row.connection_id
                ),
            ] {
                let control = rects
                    .get(&component_id)
                    .unwrap_or_else(|| panic!("missing Merge body component {component_id}"));
                assert!(control.is_positive(), "empty Merge control {component_id}");
                assert!(
                    port_rects
                        .iter()
                        .all(|port_rect| !control.intersects(*port_rect)),
                    "Merge control {component_id} overlaps a left/right port: {control:?}"
                );
            }
        }
    }

    #[test]
    fn empty_merge_body_has_a_stable_empty_state_and_minimum_estimated_height() {
        let (mut project, composition_id, _, _, _, merge_id) = fixture();
        project.connections.retain(|connection| {
            connection.to != PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
        });

        assert!(merge_layer_rows(&project, merge_id).is_empty());
        assert_eq!(estimated_node_size(&project, merge_id).y, 220.0);
        let rects = render_test_graph(&project, composition_id);
        assert!(rects
            .get(&format!("node_editor.merge_layers.empty:{merge_id}"))
            .is_some_and(egui::Rect::is_positive));
    }

    #[test]
    fn merge_blend_popup_qa_uses_foreground_clip_and_real_screen_coordinate_click() {
        fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }
        }

        #[allow(
            clippy::too_many_arguments,
            reason = "the egui frame harness keeps screen, canvas, input and authored state explicit"
        )]
        fn render_frame(
            context: &egui::Context,
            screen: egui::Rect,
            canvas_clip: egui::Rect,
            events: Vec<egui::Event>,
            time: f64,
            merge_id: Uuid,
            connection_id: Uuid,
            selected_blend: &mut BlendMode,
        ) -> egui::Rect {
            let mut selector_rect = egui::Rect::NOTHING;
            reset_test_rects();
            let raw_input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                time: Some(time),
                ..Default::default()
            };
            drop(context.run(raw_input, |context| {
                egui::Area::new(egui::Id::new("merge-popup-edge-test"))
                    .fixed_pos(egui::pos2(80.0, 128.0))
                    .show(context, |ui| {
                        // This mimics a selector still inside the Node Editor at
                        // its top edge. The ComboBox popup is a foreground Area
                        // and is therefore allowed to extend above it.
                        ui.set_clip_rect(canvas_clip);
                        selector_rect = egui::ComboBox::from_id_salt((
                            "merge-popup-edge-combo",
                            merge_id,
                            connection_id,
                        ))
                        .selected_text(blend_mode_label(*selected_blend))
                        .width(178.0)
                        .show_ui(ui, |ui| {
                            for blend_mode in AUTHORED_BLEND_MODES {
                                let selected = blend_mode == *selected_blend;
                                let option = ui.add_enabled(
                                    !selected,
                                    egui::Button::selectable(
                                        selected,
                                        blend_mode_label(blend_mode),
                                    )
                                    .frame(false),
                                );
                                register_merge_layer_popup_component(
                                    format!(
                                        "node_editor.merge_layer.blend.{}:{merge_id}:{connection_id}",
                                        blend_mode_qa_key(blend_mode)
                                    ),
                                    "node_editor_merge_layer_blend_option",
                                    option.rect,
                                    option.enabled(),
                                    ui.clip_rect(),
                                    serde_json::json!({
                                        "merge_id": merge_id,
                                        "connection_id": connection_id,
                                        "blend_mode": blend_mode_qa_key(blend_mode),
                                    }),
                                );
                                if option.clicked() {
                                    *selected_blend = blend_mode;
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .rect;
                    });
            }));
            selector_rect
        }

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 400.0));
        let canvas_clip =
            egui::Rect::from_min_max(egui::pos2(20.0, 120.0), egui::pos2(420.0, 300.0));
        let merge_id = Uuid::from_u128(0xface);
        let connection_id = Uuid::from_u128(0xcafe);
        let option_id = format!("node_editor.merge_layer.blend.overlay:{merge_id}:{connection_id}");
        let mut selected_blend = BlendMode::Normal;
        let mut frame = 0_u64;
        let mut run = |events, selected_blend: &mut BlendMode| {
            frame += 1;
            render_frame(
                &context,
                screen,
                canvas_clip,
                events,
                frame as f64 / 60.0,
                merge_id,
                connection_id,
                selected_blend,
            )
        };

        let mut selector = egui::Rect::NOTHING;
        for _ in 0..3 {
            selector = run(Vec::new(), &mut selected_blend);
        }
        assert!(
            canvas_clip.contains(selector.center()),
            "selector {selector:?} must be inside canvas {canvas_clip:?}"
        );
        selector = run(
            vec![egui::Event::PointerMoved(selector.center())],
            &mut selected_blend,
        );
        selector = run(
            vec![pointer_button(selector.center(), true)],
            &mut selected_blend,
        );
        let selector_after_open = run(
            vec![pointer_button(selector.center(), false)],
            &mut selected_blend,
        );

        let option = test_rect(&option_id).expect("foreground popup option is registered");
        let metadata = test_metadata(&option_id).expect("popup QA metadata is registered");
        assert!(option.is_positive());
        assert!(screen.contains(option.center()));
        assert!(
            option.center().y < canvas_clip.top(),
            "foreground option {option:?} from selector {selector_after_open:?} must extend above canvas {canvas_clip:?}"
        );
        assert_eq!(metadata["coordinate_space"], "screen_points");
        assert_eq!(metadata["visible_in_popup"], true);
        assert!(metadata["popup_clip_rect"]["min_y"]
            .as_f64()
            .is_some_and(|min_y| min_y < f64::from(canvas_clip.top())));

        // Use a freshly rendered option rectangle for each pointer lifecycle
        // step, matching the loopback QA bridge's real-coordinate contract.
        let _ = run(
            vec![egui::Event::PointerMoved(option.center())],
            &mut selected_blend,
        );
        let option = test_rect(&option_id).expect("popup remains open after pointer move");
        let _ = run(
            vec![pointer_button(option.center(), true)],
            &mut selected_blend,
        );
        let option = test_rect(&option_id).expect("popup remains open after pointer press");
        let _ = run(
            vec![pointer_button(option.center(), false)],
            &mut selected_blend,
        );
        assert_eq!(selected_blend, BlendMode::Overlay);
    }

    #[test]
    fn real_egui_wire_hit_selects_and_dragging_the_body_queues_disconnect() {
        let (project, _, _, _, solid_id, merge_id) = fixture();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap();
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection.id,
            },
            start: egui::pos2(120.0, 180.0),
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let midpoint =
            cubic_bezier_point(edge.start, edge.control_a, edge.control_b, edge.end, 0.5);
        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let click = vec![
            vec![egui::Event::PointerMoved(midpoint)],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        assert!(
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, click,)
                .is_empty()
        );
        assert_eq!(state.selected_connection_id, Some(connection.id));

        let escape = vec![vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]];
        assert!(
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, escape,)
                .is_empty()
        );
        assert!(state.selected_connection_id.is_none());

        assert!(run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(midpoint)],
                vec![egui::Event::PointerButton {
                    pos: midpoint,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerButton {
                    pos: midpoint,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        )
        .is_empty());
        assert_eq!(state.selected_connection_id, Some(connection.id));
        let blank = egui::pos2(32.0, 32.0);
        assert!(run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(blank)],
                vec![egui::Event::PointerButton {
                    pos: blank,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerButton {
                    pos: blank,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        )
        .is_empty());
        assert!(state.selected_connection_id.is_none());

        let dragged = midpoint + egui::vec2(0.0, 48.0);
        let drag = vec![
            vec![egui::Event::PointerMoved(midpoint)],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(dragged)],
            vec![egui::Event::PointerButton {
                pos: dragged,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let edits = run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, drag);
        assert!(
            matches!(
                edits.as_slice(),
                [QueuedNodeEdit::Atomic(NodeEdit::DisconnectConnection { connection_id })]
                    if *connection_id == connection.id
            ),
            "unexpected wire drag edits: {edits:?}; gesture: {:?}",
            state.wire_gesture
        );
    }

    #[test]
    fn connected_output_invalid_drop_and_escape_leave_project_and_history_untouched() {
        let (project, _, _, _, solid_id, merge_id) = fixture();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap();
        let source = egui::pos2(120.0, 180.0);
        let invalid_target = egui::pos2(540.0, 340.0);
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection.id,
            },
            start: source,
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
            RenderedPortKey {
                address: connection.from.clone(),
                direction: PortDirection::Output,
            },
            egui::Rect::from_center_size(source, egui::vec2(13.0, 13.0)),
        )])));

        let invalid_drop = vec![
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(invalid_target)],
            vec![egui::Event::PointerButton {
                pos: invalid_target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let mut state = NodeEditorState::default();
        let edits =
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, invalid_drop);
        assert!(edits.is_empty());
        assert!(state.normal_connect_gesture.is_none());
        assert!(state.selected_connection_id.is_none());

        let escaped = vec![
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(invalid_target)],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: invalid_target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let edits =
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, escaped);
        assert!(edits.is_empty());
        assert!(state.normal_connect_gesture.is_none());

        let mut untouched = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(project.clone());
        let undo_depth = history.undo_depth();
        assert!(!apply_queued_node_edits(
            &mut untouched,
            edits,
            &mut history,
            &mut state,
        ));
        assert_eq!(untouched, project);
        assert_eq!(history.undo_depth(), undo_depth);
    }

    #[test]
    fn overview_wire_midpoint_remains_a_body_target_when_endpoint_radii_overlap() {
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: Uuid::new_v4(),
            },
            start: egui::pos2(100.0, 100.0),
            control_a: egui::pos2(106.0, 100.0),
            control_b: egui::pos2(114.0, 100.0),
            end: egui::pos2(120.0, 100.0),
        };
        assert_eq!(
            rendered_wire_drag_kind(&edge, egui::pos2(110.0, 100.0)),
            NodeEditorWireDragKind::Disconnect
        );
        assert_eq!(
            rendered_wire_drag_kind(&edge, edge.start),
            NodeEditorWireDragKind::ReconnectSource
        );
        assert_eq!(
            rendered_wire_drag_kind(&edge, edge.end),
            NodeEditorWireDragKind::ReconnectTarget
        );
    }

    #[test]
    fn endpoint_drag_reconnects_through_real_pointer_frames_without_changing_wire_identity() {
        let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
        let mut alternate = generator_node(
            "Alternate",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        alternate.ui_position = [250.0, 520.0];
        let alternate_id = alternate.id;
        project.add_node(alternate);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), alternate_id)
            .unwrap();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap()
            .clone();
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection.id,
            },
            start: egui::pos2(120.0, 180.0),
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let alternate_position = egui::pos2(480.0, 260.0);
        let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(alternate_id), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
            },
            egui::Rect::from_center_size(alternate_position, egui::vec2(14.0, 14.0)),
        )])));
        let mut state = NodeEditorState::default();
        let edits = run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(edge.start)],
                vec![egui::Event::PointerButton {
                    pos: edge.start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerMoved(alternate_position)],
                vec![egui::Event::PointerButton {
                    pos: alternate_position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        );
        let [QueuedNodeEdit::Atomic(NodeEdit::ReconnectConnection {
            connection_id,
            from,
            to,
        })] = edits.as_slice()
        else {
            panic!("endpoint drag did not queue one reconnect: {edits:?}");
        };
        assert_eq!(*connection_id, connection.id);
        assert_eq!(from.owner, PortOwner::Node(alternate_id));
        assert_eq!(*to, connection.to);
        assert!(apply_edit(
            &mut project,
            NodeEdit::ReconnectConnection {
                connection_id: *connection_id,
                from: from.clone(),
                to: to.clone(),
            },
        ));
        let reconnected = project
            .connections
            .iter()
            .find(|candidate| candidate.id == connection.id)
            .unwrap();
        assert_eq!(reconnected.from.owner, PortOwner::Node(alternate_id));
        assert_eq!(reconnected.to, connection.to);
        assert_eq!(reconnected.order, connection.order);
    }

    #[test]
    fn operation_node_splice_preserves_downstream_uuid_order_and_target() {
        let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap()
            .clone();
        let plugins = PluginManager::default();
        let mut blur = plugins.create_effect_operation_node("blur").unwrap();
        blur.ui_position = [610.0, 500.0];
        let blur_id = blur.id;
        project.add_node(blur);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), blur_id)
            .unwrap();
        assert!(splice_existing_node_on_connection(
            &mut project,
            connection.id,
            blur_id,
        ));
        let downstream = project
            .connections
            .iter()
            .find(|candidate| candidate.id == connection.id)
            .unwrap();
        assert_eq!(downstream.from.owner, PortOwner::Node(blur_id));
        assert_eq!(downstream.to, connection.to);
        assert_eq!(downstream.order, connection.order);

        let second_connection = project
            .connections
            .iter()
            .find(|candidate| candidate.to.owner == PortOwner::Node(blur_id))
            .unwrap()
            .clone();
        let second_blur = plugins.create_effect_operation_node("blur").unwrap();
        let second_blur_id = second_blur.id;
        assert!(insert_node_on_connection(
            &mut project,
            second_connection.id,
            second_blur,
            egui::pos2(560.0, 440.0),
            composition_id,
        ));
        assert_eq!(
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == second_connection.id)
                .unwrap()
                .to,
            second_connection.to
        );
        assert_eq!(
            project.find_node_container(second_blur_id),
            Some(NodeContainer::Clip(clip_id))
        );
    }

    fn queued_property_edit(
        owner: PortOwner,
        key: &str,
        time: f64,
        value: PropertyValue,
        finished: bool,
    ) -> QueuedNodeEdit {
        QueuedNodeEdit::Continuous {
            pending: NodeEditorPendingEdit {
                owner,
                key: key.to_string(),
            },
            edit: Some(NodeEdit::SetProperty {
                owner,
                key: key.to_string(),
                time,
                value,
            }),
            finished,
        }
    }

    fn queued_finish(owner: PortOwner, key: &str) -> QueuedNodeEdit {
        QueuedNodeEdit::Continuous {
            pending: NodeEditorPendingEdit {
                owner,
                key: key.to_string(),
            },
            edit: None,
            finished: true,
        }
    }

    fn assert_single_gesture_undo_redo(
        history: &mut HistoryManager,
        initial: &Project,
        edited: &Project,
    ) {
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(edited), Some(initial.clone()));
        assert_eq!(history.redo(initial), Some(edited.clone()));
    }

    #[derive(Clone, Copy)]
    struct HierarchyFixtureIds {
        composition: Uuid,
        track: Uuid,
        empty_track: Uuid,
        clip: Uuid,
        sibling_clip: Uuid,
        empty_clip: Uuid,
        solid: Uuid,
        merge: Uuid,
        text: Uuid,
        text_fill: Uuid,
        shape: Uuid,
        shape_fill: Uuid,
        composition_node: Uuid,
    }

    fn adversarial_hierarchy_fixture() -> (Project, HierarchyFixtureIds) {
        let (mut project, composition, track, clip, solid, merge) = fixture();
        let overlapping_position = [420.0, 300.0];
        project.get_node_mut(solid).unwrap().ui_position = overlapping_position;
        project.get_node_mut(merge).unwrap().ui_position = overlapping_position;

        let sibling_clip = Uuid::from_u128(0x7_001);
        let mut clip_b = library::model::Clip::new("Sibling Clip", 0.0, 5.0);
        clip_b.id = sibling_clip;
        clip_b.ui_position = [260.0, 260.0];
        clip_b.ui_size = [360.0, 220.0];
        project.add_clip(clip_b);
        project.attach_clip_to_track(track, sibling_clip).unwrap();

        let text = Uuid::from_u128(0x7_002);
        let mut text_node = generator_node(
            "Tall Text",
            GeneratorNodeRequest::Text {
                text: "A deliberately tall text Node".to_string(),
                font: "Arial".to_string(),
            },
        );
        text_node.id = text;
        text_node.ui_position = overlapping_position;
        project.add_node(text_node);
        project
            .attach_node_to_container(NodeContainer::Clip(sibling_clip), text)
            .unwrap();
        let text_fill = Uuid::from_u128(0x7_007);
        let mut text_fill_node = PluginManager::default()
            .create_style_operation_node("fill")
            .unwrap();
        text_fill_node.id = text_fill;
        text_fill_node.ui_position = overlapping_position;
        project.add_node(text_fill_node);
        project
            .attach_node_to_container(NodeContainer::Clip(sibling_clip), text_fill)
            .unwrap();
        project.get_clip_mut(sibling_clip).unwrap().output_node_id = Some(text_fill);

        let empty_clip = Uuid::from_u128(0x7_003);
        let mut collapsed_clip = library::model::Clip::new("Collapsed Empty", 0.0, 5.0);
        collapsed_clip.id = empty_clip;
        collapsed_clip.ui_collapsed = true;
        collapsed_clip.ui_position = [260.0, 260.0];
        project.add_clip(collapsed_clip);
        project.attach_clip_to_track(track, empty_clip).unwrap();

        let shape = Uuid::from_u128(0x7_004);
        let mut shape_node = generator_node(
            "Track Shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        );
        shape_node.id = shape;
        shape_node.ui_position = overlapping_position;
        project.add_node(shape_node);
        project
            .attach_node_to_container(NodeContainer::Track(track), shape)
            .unwrap();
        let shape_fill = Uuid::from_u128(0x7_008);
        let mut shape_fill_node = PluginManager::default()
            .create_style_operation_node("fill")
            .unwrap();
        shape_fill_node.id = shape_fill;
        shape_fill_node.ui_position = overlapping_position;
        project.add_node(shape_fill_node);
        project
            .attach_node_to_container(NodeContainer::Track(track), shape_fill)
            .unwrap();

        let composition_node = Uuid::from_u128(0x7_005);
        let mut root_merge = Node::new_merge("Composition Merge");
        root_merge.id = composition_node;
        root_merge.ui_position = overlapping_position;
        project.add_node(root_merge);
        project
            .attach_node_to_container(NodeContainer::Composition(composition), composition_node)
            .unwrap();

        let empty_track = Uuid::from_u128(0x7_006);
        let mut collapsed_track = library::model::Track::new("Collapsed Empty Track");
        collapsed_track.id = empty_track;
        collapsed_track.ui_collapsed = true;
        collapsed_track.ui_position = [110.0, 140.0];
        project.add_track(collapsed_track);
        project
            .attach_track_to_composition(composition, empty_track)
            .unwrap();

        for (source, fill) in [(text, text_fill), (shape, shape_fill)] {
            project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(fill), SHAPE_INPUT_PORT),
                )
                .unwrap();
            project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(fill), IMAGE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(merge), MERGE_IMAGES_PORT),
                )
                .unwrap();
        }
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(merge), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(composition_node), MERGE_IMAGES_PORT),
            )
            .unwrap();

        (
            project,
            HierarchyFixtureIds {
                composition,
                track,
                empty_track,
                clip,
                sibling_clip,
                empty_clip,
                solid,
                merge,
                text,
                text_fill,
                shape,
                shape_fill,
                composition_node,
            },
        )
    }

    fn render_test_graph_with_context_menu_exclusions(
        project: &Project,
        composition_id: Uuid,
    ) -> (
        HashMap<String, egui::Rect>,
        Vec<egui::Rect>,
        egui::emath::TSTransform,
        Vec<ContainerVisual>,
    ) {
        let context = egui::Context::default();
        let (mut snarl, containers) = build_snarl(project, composition_id);
        let mut final_context_menu_exclusion_rects = Vec::new();
        let mut final_to_global = egui::emath::TSTransform::default();
        reset_test_rects();

        for frame in 0..5 {
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 1200.0),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            drop(context.run(raw_input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
                    let mut wire_context_request = None;
                    let mut context_menu_exclusion_rects = Vec::new();
                    let mut to_global = egui::emath::TSTransform::default();
                    let mut canvas_clip = ui.clip_rect();
                    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                    let mut viewer = ProjectNodeViewer {
                        project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        pending_selection: &mut selection,
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::clone(&rendered_ports),
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                    };
                    let style = SnarlStyle {
                        collapsible: Some(false),
                        min_scale: Some(0.05),
                        max_scale: Some(1.0),
                        ..Default::default()
                    };
                    snarl.show(
                        &mut viewer,
                        &style,
                        egui::Id::new(("hierarchical-layout-test", composition_id)),
                        ui,
                    );
                    drop(viewer);
                    final_context_menu_exclusion_rects = context_menu_exclusion_rects;
                    final_to_global = to_global;
                    for container in &containers {
                        register_container_chrome(container, to_global, canvas_clip, project, 0.0);
                    }
                    register_rendered_edges(project, &rendered_ports, canvas_clip, None);
                });
            }));
        }
        (
            test_rects(),
            final_context_menu_exclusion_rects,
            final_to_global,
            containers,
        )
    }

    fn render_test_graph(project: &Project, composition_id: Uuid) -> HashMap<String, egui::Rect> {
        render_test_graph_with_context_menu_exclusions(project, composition_id).0
    }

    #[test]
    fn text_add_inserts_one_clean_graph_without_replacing_the_clip_output() {
        let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(
            &mut project,
            composition_id,
            &initial_layout
        ));
        assert!(!layout_needs_reflow(&project, composition_id));

        let factory = style_graph_factory();
        let graph = factory
            .create_text_graph("Hello", "Arial", 1920, 1080)
            .unwrap();
        let consumer_id = graph.output_node_id.expect("Text factory sink");
        let source_id = graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.content(),
                    NodeContent::Generator(GeneratorContent::Text)
                )
            })
            .unwrap()
            .id;
        let fill_id = graph
            .nodes
            .iter()
            .find(|node| plugin_operation_component(node) == Some("fill"))
            .unwrap()
            .id;
        let connection = graph.connections[0].clone();
        let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();

        let mut laid_out = graph.clone();
        layout_detached_node_graph(&project, &mut laid_out);
        assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);
        let relative_positions = laid_out
            .nodes
            .iter()
            .map(|node| (node.id, node.ui_position))
            .collect::<HashMap<_, _>>();

        let clip = project.get_clip(clip_id).unwrap();
        let desired = egui::pos2(
            clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
            clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
        );
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(insert_prebuilt_graph(
            &mut project,
            desired,
            graph,
            composition_id
        ));
        history.push_project_state(project.clone());

        let clip = project.get_clip(clip_id).unwrap();
        assert_eq!(clip.output_node_id, Some(merge_id));
        assert_eq!(
            &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
            bundled_ids.as_slice()
        );
        assert_eq!(
            project.find_node_container(consumer_id),
            Some(NodeContainer::Clip(clip_id))
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == connection.id),
            Some(&connection)
        );

        let shape_output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .unwrap();
        let shape_input = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                PortDirection::Input,
            )
            .unwrap();
        let image_output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .unwrap();
        assert_eq!(shape_output.data_type, PortDataType::Shape);
        assert_eq!(shape_input.data_type, PortDataType::Shape);
        assert_eq!(image_output.data_type, PortDataType::Image);

        let first_id = bundled_ids[0];
        let first = project.get_node(first_id).unwrap().ui_position;
        let first_relative = relative_positions[&first_id];
        let translation = [first[0] - first_relative[0], first[1] - first_relative[1]];
        for node_id in &bundled_ids {
            let inserted = project.get_node(*node_id).unwrap().ui_position;
            let relative = relative_positions[node_id];
            assert!((inserted[0] - relative[0] - translation[0]).abs() < 0.01);
            assert!((inserted[1] - relative[1] - translation[1]).abs() < 0.01);
        }
        assert!(!layout_needs_reflow(&project, composition_id));

        let rects = render_test_graph(&project, composition_id);
        for id in [
            format!("node_editor.port.node:{source_id}.output:{SHAPE_OUTPUT_PORT}"),
            format!("node_editor.port.node:{fill_id}.input:{SHAPE_INPUT_PORT}"),
            format!("node_editor.port.node:{fill_id}.output:{IMAGE_OUTPUT_PORT}"),
            format!("node_editor.edge:{}", connection.id),
        ] {
            assert!(
                rects.get(&id).is_some_and(egui::Rect::is_positive),
                "missing visible typed Shape/Image graph component {id}"
            );
        }

        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn shape_add_preserves_none_output_and_fill_stroke_order_without_overlap() {
        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(
            &mut project,
            composition_id,
            &initial_layout
        ));
        assert_eq!(project.get_clip(clip_id).unwrap().output_node_id, None);

        let factory = style_graph_factory();
        let graph = factory
            .create_shape_graph("M0 0 H100 V100 Z", 1920, 1080, 100, 100)
            .unwrap();
        let consumer_id = graph.output_node_id.expect("Shape factory sink");
        let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let connection_ids = graph
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        let mut laid_out = graph.clone();
        layout_detached_node_graph(&project, &mut laid_out);
        assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);

        let clip = project.get_clip(clip_id).unwrap();
        let desired = egui::pos2(
            clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
            clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
        );
        assert!(insert_prebuilt_graph(
            &mut project,
            desired,
            graph,
            composition_id
        ));

        let clip = project.get_clip(clip_id).unwrap();
        assert_eq!(clip.output_node_id, None);
        assert_eq!(
            &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
            bundled_ids.as_slice()
        );
        let appended = project
            .connections
            .iter()
            .filter(|connection| {
                connection_ids.contains(&connection.id)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(consumer_id), MERGE_IMAGES_PORT)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            appended
                .iter()
                .map(|connection| connection.order)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            appended
                .iter()
                .map(|connection| {
                    let PortOwner::Node(node_id) = connection.from.owner else {
                        panic!("Style source must be a Node")
                    };
                    plugin_operation_component(project.get_node(node_id).unwrap()).unwrap()
                })
                .collect::<Vec<_>>(),
            vec!["fill", "stroke"]
        );
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn standalone_style_add_has_shape_input_image_output_and_failed_graph_add_is_atomic() {
        let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let factory = style_graph_factory();
        let plugins = factory.get_plugin_manager();

        let fill = plugins.create_style_operation_node("fill").unwrap();
        let fill_id = fill.id;
        assert!(insert_prebuilt_graph(
            &mut project,
            egui::pos2(500.0, 350.0),
            NodeGraphBundle::new(vec![fill], Vec::new(), None),
            composition_id,
        ));
        assert_eq!(
            project.get_clip(clip_id).unwrap().output_node_id,
            Some(merge_id)
        );
        assert_eq!(
            project.find_node_container(fill_id),
            Some(NodeContainer::Clip(clip_id))
        );
        assert_eq!(
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                    PortDirection::Output,
                )
                .unwrap()
                .data_type,
            PortDataType::Image
        );
        assert_eq!(
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                    PortDirection::Input,
                )
                .unwrap()
                .data_type,
            PortDataType::Shape
        );

        let stroke = plugins.create_style_operation_node("stroke").unwrap();
        let stroke_id = stroke.id;
        let width = plugin_operation_property_definition(&plugins, &stroke, "width")
            .expect("runtime descriptor width metadata");
        assert!(matches!(
            width.ui_type(),
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix,
                ..
            } if suffix == "px"
        ));
        let join = plugin_operation_property_definition(&plugins, &stroke, "join")
            .expect("runtime descriptor enum metadata");
        assert!(matches!(
            join.ui_type(),
            PropertyUiType::Dropdown { options }
                if options.iter().map(String::as_str).eq(["Miter", "Round", "Bevel"])
        ));
        assert!(insert_prebuilt_graph(
            &mut project,
            egui::pos2(500.0, 350.0),
            NodeGraphBundle::new(vec![stroke], Vec::new(), None),
            composition_id,
        ));
        assert_eq!(
            project.get_clip(clip_id).unwrap().output_node_id,
            Some(merge_id)
        );
        assert_eq!(
            project.find_node_container(stroke_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let rects = render_test_graph(&project, composition_id);
        for node_id in [fill_id, stroke_id] {
            let output = format!("node_editor.port.node:{node_id}.output:{IMAGE_OUTPUT_PORT}");
            assert!(rects.get(&output).is_some_and(egui::Rect::is_positive));
        }

        let mut invalid = factory
            .create_text_graph("duplicate", "Arial", 1920, 1080)
            .unwrap();
        invalid.nodes[0].id = merge_id;
        let before = project.clone();
        assert!(!insert_prebuilt_graph(
            &mut project,
            egui::pos2(500.0, 350.0),
            invalid,
            composition_id,
        ));
        assert_eq!(
            project, before,
            "failed insertion must not partially mutate Project"
        );
    }

    #[test]
    fn effect_operation_add_is_ltr_typed_atomic_and_preserves_the_clip_output() {
        let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(
            &mut project,
            composition_id,
            &initial_layout
        ));

        let factory = style_graph_factory();
        let plugins = factory.get_plugin_manager();
        assert!(plugins
            .get_available_effects()
            .iter()
            .any(|(id, _, _)| id == "blur"));
        let source = factory
            .create_solid_node(
                library::model::frame::color::Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                1920,
                1080,
            )
            .unwrap();
        let effect = plugins.create_effect_operation_node("blur").unwrap();
        let source_id = source.id;
        let effect_id = effect.id;
        let sigma_x = plugin_operation_property_definition(&plugins, &effect, "sigma_x")
            .expect("Blur numeric metadata");
        assert!(matches!(
            sigma_x.ui_type(),
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 0.1,
                suffix,
                min_hard_limit: true,
                max_hard_limit: false,
            } if suffix == "px"
        ));
        let tile_mode = plugin_operation_property_definition(&plugins, &effect, "tile_mode")
            .expect("Blur enum metadata");
        assert!(matches!(
            tile_mode.ui_type(),
            PropertyUiType::Dropdown { options }
                if options.iter().map(String::as_str).eq(["clamp", "repeat", "mirror", "decal"])
        ));

        let wire = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
            0,
        );
        let graph = NodeGraphBundle::new(vec![source, effect], vec![wire.clone()], Some(effect_id));
        let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let mut laid_out = graph.clone();
        layout_detached_node_graph(&project, &mut laid_out);
        assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);

        let clip = project.get_clip(clip_id).unwrap();
        let desired = egui::pos2(
            clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
            clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
        );
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(insert_prebuilt_graph(
            &mut project,
            desired,
            graph,
            composition_id,
        ));
        history.push_project_state(project.clone());

        let clip = project.get_clip(clip_id).unwrap();
        assert_eq!(clip.output_node_id, Some(merge_id));
        assert_eq!(
            &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
            bundled_ids.as_slice()
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == wire.id),
            Some(&wire)
        );
        for (port, direction) in [
            (IMAGE_INPUT_PORT, PortDirection::Input),
            (IMAGE_OUTPUT_PORT, PortDirection::Output),
        ] {
            assert_eq!(
                project
                    .port_definition(
                        &PortAddress::new(PortOwner::Node(effect_id), port),
                        direction,
                    )
                    .unwrap()
                    .data_type,
                PortDataType::Image
            );
        }
        assert!(!layout_needs_reflow(&project, composition_id));

        let rects = render_test_graph(&project, composition_id);
        for id in [
            format!("node_editor.port.node:{effect_id}.input:{IMAGE_INPUT_PORT}"),
            format!("node_editor.port.node:{effect_id}.output:{IMAGE_OUTPUT_PORT}"),
            format!("node_editor.edge:{}", wire.id),
        ] {
            assert!(
                rects.get(&id).is_some_and(egui::Rect::is_positive),
                "missing visible Effect graph component {id}"
            );
        }
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);

        let mut duplicate = plugins.create_effect_operation_node("blur").unwrap();
        duplicate.id = merge_id;
        let before_failure = project.clone();
        assert!(!insert_prebuilt_graph(
            &mut project,
            desired,
            NodeGraphBundle::new(vec![duplicate], Vec::new(), None),
            composition_id,
        ));
        assert_eq!(project, before_failure);
    }

    #[test]
    fn effector_operation_nodes_and_menu_use_the_authoritative_descriptor() {
        let factory = style_graph_factory();
        let plugins = factory.get_plugin_manager();
        let menu_entries = available_effector_menu_entries(plugins.as_ref());
        assert!(menu_entries.contains(&("transform".to_string(), "Transform".to_string())));
        assert!(menu_entries.contains(&("opacity".to_string(), "Opacity".to_string())));
        assert!(menu_entries
            .windows(2)
            .all(|entries| entries[0].1 <= entries[1].1));

        for component_id in ["transform", "opacity"] {
            let descriptor = plugins
                .operation_descriptor(EFFECTOR_CATEGORY, component_id, EFFECTOR_APPLY_OPERATION)
                .unwrap();
            let node = plugins
                .create_effector_operation_node(component_id)
                .unwrap();
            assert_eq!(
                node.properties()
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<BTreeSet<_>>(),
                descriptor
                    .properties()
                    .iter()
                    .map(PropertyDefinition::name)
                    .collect::<BTreeSet<_>>()
            );
            for definition in descriptor.properties() {
                assert_eq!(
                    node.properties()
                        .get(definition.name())
                        .and_then(|property| property.evaluate_at(0.0).ok()),
                    Some(definition.default_value().clone()),
                    "{component_id}.{} was not initialized by its descriptor factory",
                    definition.name(),
                );
            }
        }
        let transform = plugins.create_effector_operation_node("transform").unwrap();
        assert_eq!(
            transform
                .properties()
                .get("target")
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(PropertyValue::String("Block".to_string()))
        );
        let opacity = plugins.create_effector_operation_node("opacity").unwrap();
        assert_eq!(
            opacity
                .properties()
                .get("mode")
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(PropertyValue::String("Set".to_string()))
        );
        assert_eq!(
            opacity
                .properties()
                .get("target")
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(PropertyValue::String("Block".to_string()))
        );
    }

    #[test]
    fn node_editor_effector_control_responds_to_real_pointer_drag() {
        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let plugins = PluginManager::default();
        let mut effector = plugins.create_effector_operation_node("transform").unwrap();
        effector.ui_position = [520.0, 390.0];
        let effector_id = effector.id;
        project.add_node(effector);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), effector_id)
            .unwrap();
        let layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(&mut project, composition_id, &layout));
        assert!(!layout_needs_reflow(&project, composition_id));
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let component_id = format!("node_editor.property.node:{effector_id}:tx");
        let mut queued = Vec::new();
        reset_test_rects();

        let mut frames = vec![Vec::new(); 5];
        for (frame, events) in frames.drain(..).enumerate() {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: Some(&plugins),
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("effector-real-event", composition_id)),
                            ui,
                        );
                        queued.extend(edits);
                    });
                },
            );
            drop(output);
        }
        let rect = test_rect(&component_id).expect("rendered Transform tx control");
        assert!(rect.is_positive());
        let start = rect.center();
        let end = start + egui::vec2(52.0, 0.0);
        let event_frames = [
            vec![egui::Event::PointerMoved(start)],
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for (offset, events) in event_frames.into_iter().enumerate() {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((offset + 5) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: Some(&plugins),
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("effector-real-event", composition_id)),
                            ui,
                        );
                        queued.extend(edits);
                    });
                },
            );
            drop(output);
        }

        assert!(
            queued.iter().any(|edit| matches!(
                edit,
                QueuedNodeEdit::Continuous {
                    edit: Some(NodeEdit::SetProperty {
                        owner: PortOwner::Node(id),
                        key,
                        value: PropertyValue::Number(value),
                        ..
                    }),
                    ..
                } if *id == effector_id && key == "tx" && value.into_inner() > 0.0
            )),
            "real pointer drag over {rect:?} did not edit tx: {queued:#?}"
        );
        assert!(queued.iter().any(|edit| matches!(
            edit,
            QueuedNodeEdit::Continuous {
                pending,
                finished: true,
                ..
            } if pending.owner == PortOwner::Node(effector_id) && pending.key == "tx"
        )));
    }

    #[test]
    fn extreme_zoom_transform_and_adaptive_grid_remain_finite_and_bounded() {
        let style = node_editor_snarl_style();
        assert_eq!(style.min_scale, Some(0.0065));
        assert_eq!(style.max_scale, Some(1.25));

        let mut corrupt =
            egui::emath::TSTransform::new(egui::vec2(f32::NAN, f32::INFINITY), f32::NEG_INFINITY);
        sanitize_node_editor_transform(&mut corrupt);
        assert_eq!(corrupt, egui::emath::TSTransform::IDENTITY);
        assert!(corrupt.is_valid());
        assert!(corrupt.translation.y.is_finite());

        let mut extreme = egui::emath::TSTransform::new(
            egui::vec2(20_000_000.0, -20_000_000.0),
            NODE_EDITOR_MIN_SCALE / 10.0,
        );
        sanitize_node_editor_transform(&mut extreme);
        assert_eq!(extreme.scaling, NODE_EDITOR_MIN_SCALE);
        assert_eq!(extreme.translation.x, NODE_EDITOR_MAX_TRANSLATION);
        assert_eq!(extreme.translation.y, -NODE_EDITOR_MAX_TRANSLATION);

        let transform =
            egui::emath::TSTransform::new(egui::vec2(347.0, -73.0), NODE_EDITOR_MIN_SCALE);
        let graph_position = egui::pos2(500_000.0, -250_000.0);
        let screen_position = transform * graph_position;
        let round_trip = transform.inverse() * screen_position;
        assert!(screen_position.x.is_finite() && screen_position.y.is_finite());
        assert!(round_trip.distance(graph_position) < 0.1);
        assert!(!node_editor_details_visible(NODE_EDITOR_MIN_SCALE));
        assert!(node_editor_details_visible(NODE_EDITOR_DETAIL_SCALE));
        assert!(
            (screen_stroke_in_graph_units(1.65, NODE_EDITOR_MIN_SCALE) * NODE_EDITOR_MIN_SCALE
                - 1.65)
                .abs()
                < 1.0e-5
        );

        let screen_wire = [
            egui::pos2(10.0, 20.0),
            egui::pos2(35.0, 20.0),
            egui::pos2(65.0, 80.0),
            egui::pos2(90.0, 80.0),
        ];
        let graph_wire =
            overview_wire_graph_points(screen_wire, transform).expect("finite overview wire");
        for (screen, graph) in screen_wire.into_iter().zip(graph_wire) {
            assert!((transform * graph).distance(screen) < 0.001);
        }
        assert!(overview_wire_graph_points(
            screen_wire,
            egui::emath::TSTransform::new(egui::Vec2::ZERO, 0.0),
        )
        .is_none());

        for scale in [
            NODE_EDITOR_MIN_SCALE,
            0.01,
            0.02,
            0.1,
            NODE_EDITOR_DETAIL_SCALE,
            1.0,
            NODE_EDITOR_MAX_SCALE,
        ] {
            let spacing = adaptive_grid_spacing(scale);
            let screen_spacing = spacing * scale;
            assert!(spacing.is_finite() && spacing > 0.0);
            assert!(
                (GRID_TARGET_SCREEN_SPACING..=GRID_TARGET_SCREEN_SPACING * 2.5 + 0.01)
                    .contains(&screen_spacing),
                "scale={scale}, spacing={spacing}, screen_spacing={screen_spacing}"
            );
            let approximate_line_count =
                (1800.0 / screen_spacing).ceil() + (1200.0 / screen_spacing).ceil();
            assert!(approximate_line_count < 70.0);
        }
    }

    #[test]
    fn overview_wire_survives_the_real_egui_layer_transform_in_screen_space() {
        let context = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let to_global =
            egui::emath::TSTransform::new(egui::vec2(420.0, 310.0), NODE_EDITOR_MIN_SCALE);
        let start = egui::pos2(250.0, 300.0);
        let end = egui::pos2(750.0, 500.0);
        let from = PortAddress::new(PortOwner::Node(Uuid::from_u128(0x901)), "image");
        let to = PortAddress::new(PortOwner::Node(Uuid::from_u128(0x902)), "image");
        let ports = HashMap::from([
            (
                RenderedPortKey {
                    address: from.clone(),
                    direction: PortDirection::Output,
                },
                egui::Rect::from_center_size(start, egui::Vec2::ZERO),
            ),
            (
                RenderedPortKey {
                    address: to.clone(),
                    direction: PortDirection::Input,
                },
                egui::Rect::from_center_size(end, egui::Vec2::ZERO),
            ),
        ]);

        let output = context.run(
            egui::RawInput {
                screen_rect: Some(canvas),
                ..Default::default()
            },
            |context| {
                let layer = egui::LayerId::new(
                    egui::Order::Middle,
                    egui::Id::new("overview-wire-transform-test"),
                );
                context.set_transform_layer(layer, to_global);
                let painter =
                    egui::Painter::new(context.clone(), layer, to_global.inverse() * canvas);
                let _ = register_edge_component(
                    EdgeComponent {
                        id: "node_editor.edge:overview-wire-transform-test".to_string(),
                        kind: RenderedEdgeKind::ProjectConnection {
                            connection_id: Uuid::from_u128(0x8_001),
                        },
                        from: &from,
                        to: &to,
                        wire_color: pin_color(PortDataType::Image),
                        authored_order: None,
                        back_to_front_index: None,
                        layer_count: None,
                        authored_blend_mode: None,
                        authored_blend_available: false,
                    },
                    &ports,
                    canvas,
                    Some(OverviewWirePainter {
                        painter: &painter,
                        to_global,
                    }),
                );
            },
        );

        let (clip_rect, wire) = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::CubicBezier(wire) => Some((clipped.clip_rect, wire)),
                _ => None,
            })
            .expect("overview CubicBezier in final egui output");
        let expected_frame = ((end.x - start.x).abs() * 0.45).clamp(2.0, 110.0);
        let expected = [
            start,
            start + egui::vec2(expected_frame, 0.0),
            end - egui::vec2(expected_frame, 0.0),
            end,
        ];
        for (actual, expected) in wire.points.iter().zip(expected) {
            assert!(actual.distance(expected) < 0.01);
        }
        assert!((wire.stroke.width - 1.65).abs() < 0.001);
        assert!(clip_rect.min.distance(canvas.min) < 0.01);
        assert!(clip_rect.max.distance(canvas.max) < 0.01);
    }

    #[test]
    fn overview_canvas_keeps_pan_gestures_while_precision_controls_are_disabled() {
        let (project, composition_id, _, _, _, _) = fixture();
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let zoom_position = egui::pos2(500.0, 400.0);
        let drag_start = egui::pos2(100.0, 100.0);
        let drag_end = egui::pos2(220.0, 160.0);
        let command_modifiers = egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ..egui::Modifiers::NONE
        };
        let frames = [
            (Vec::new(), egui::Modifiers::NONE),
            (
                vec![
                    egui::Event::PointerMoved(zoom_position),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -10_000.0),
                        modifiers: command_modifiers,
                    },
                ],
                command_modifiers,
            ),
            (
                vec![
                    egui::Event::PointerMoved(drag_start),
                    egui::Event::PointerButton {
                        pos: drag_start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
            ),
            (
                vec![egui::Event::PointerMoved(drag_end)],
                egui::Modifiers::NONE,
            ),
            (
                vec![egui::Event::PointerButton {
                    pos: drag_end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                egui::Modifiers::NONE,
            ),
        ];
        let mut transforms = Vec::new();
        let mut node_editor_state = NodeEditorState::default();

        for (frame, (events, modifiers)) in frames.into_iter().enumerate() {
            let mut rendered_transform = egui::emath::TSTransform::IDENTITY;
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports,
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("overview-pan-test", composition_id)),
                            ui,
                        );
                        drop(viewer);
                        let resize_edits = container_resize_interactions(
                            ui,
                            &project,
                            &containers,
                            to_global,
                            canvas_clip,
                            &mut node_editor_state,
                        );
                        assert!(edits.is_empty());
                        assert!(resize_edits.is_empty());
                        rendered_transform = to_global;
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            transforms.push(rendered_transform);
        }

        let zoomed = transforms[1];
        let dragged = transforms[3];
        assert!((zoomed.scaling - NODE_EDITOR_MIN_SCALE).abs() < f32::EPSILON);
        assert_eq!(dragged.scaling, zoomed.scaling);
        assert!(!node_editor_resize_interactions_enabled(zoomed.scaling));
        assert!(!node_editor_port_interactions_enabled(zoomed.scaling));
        let pan_delta = dragged.translation - zoomed.translation;
        assert!((pan_delta.x - (drag_end.x - drag_start.x)).abs() < 1.0);
        assert!((pan_delta.y - (drag_end.y - drag_start.y)).abs() < 1.0);
        assert!(node_editor_state.container_resize.is_none());
    }

    #[test]
    fn canvas_qa_metadata_exposes_the_final_clamped_transform_and_lod_gates() {
        let composition_id = Uuid::from_u128(0xCA_11_A5);
        let metadata = node_editor_canvas_metadata(
            composition_id,
            egui::emath::TSTransform::new(egui::vec2(321.5, -87.25), NODE_EDITOR_MIN_SCALE / 10.0),
        );
        assert_eq!(metadata["composition_id"], composition_id.to_string());
        assert_eq!(metadata["scale"], NODE_EDITOR_MIN_SCALE);
        assert_eq!(metadata["translation"]["x"], 321.5);
        assert_eq!(metadata["translation"]["y"], -87.25);
        assert_eq!(metadata["min_scale"], NODE_EDITOR_MIN_SCALE);
        assert_eq!(metadata["max_scale"], NODE_EDITOR_MAX_SCALE);
        assert_eq!(metadata["detail_enabled"], false);
        assert_eq!(metadata["port_interaction_enabled"], false);
        assert_eq!(metadata["resize_interaction_enabled"], false);
    }

    #[test]
    fn active_knife_owns_the_canvas_transform_instead_of_panning_the_scene() {
        let locked = egui::emath::TSTransform::new(egui::vec2(120.0, 240.0), 0.25);
        let mut scene_pan =
            egui::emath::TSTransform::new(egui::vec2(1_920.0, -480.0), locked.scaling);
        resolve_node_editor_transform(&mut scene_pan, Some(locked));
        assert_eq!(scene_pan, locked);

        let mut normal_pan =
            egui::emath::TSTransform::new(egui::vec2(1_920.0, -480.0), locked.scaling);
        resolve_node_editor_transform(&mut normal_pan, None);
        assert_eq!(normal_pan.translation, egui::vec2(1_920.0, -480.0));
    }

    #[test]
    fn overview_port_qa_rect_matches_the_real_reconnect_drop_hit_test() {
        let scale = NODE_EDITOR_DETAIL_SCALE * 0.5;
        assert!(!node_editor_port_interactions_enabled(scale));
        let to_global = egui::emath::TSTransform::new(egui::vec2(410.0, 290.0), scale);
        let graph_position = egui::pos2(120.0, 80.0);
        let rendered_port_rect =
            to_global * egui::Rect::from_center_size(graph_position, egui::Vec2::ZERO);
        let drop_rect = wire_port_drop_rect(rendered_port_rect);
        let position = rendered_port_rect.center();
        assert_eq!(drop_rect.size(), egui::vec2(10.0, 10.0));
        assert!(drop_rect.contains(position));

        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        assert!(clipped_qa_rect(drop_rect, canvas).is_positive());
        let address = PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT);
        let ports = HashMap::from([(
            RenderedPortKey {
                address: address.clone(),
                direction: PortDirection::Output,
            },
            rendered_port_rect,
        )]);
        assert_eq!(
            rendered_port_at_position(&ports, PortDirection::Output, position, canvas),
            Some(address)
        );
        assert!(
            rendered_normal_port_at_position(&ports, position, canvas).is_none(),
            "overview reconnect drop targets must not steal normal Snarl connection gestures"
        );
        let detailed_ports = HashMap::from([(
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
            },
            egui::Rect::from_center_size(position, egui::vec2(13.0, 13.0)),
        )]);
        assert!(rendered_normal_port_at_position(&detailed_ports, position, canvas).is_some());

        let offscreen = egui::Rect::from_center_size(
            egui::pos2(canvas.right() + 10.0, 400.0),
            egui::Vec2::ZERO,
        );
        assert!(!clipped_qa_rect(wire_port_drop_rect(offscreen), canvas).is_positive());
        let offscreen_ports = HashMap::from([(
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
            },
            offscreen,
        )]);
        assert!(rendered_port_at_position(
            &offscreen_ports,
            PortDirection::Output,
            offscreen.center(),
            canvas,
        )
        .is_none());
    }

    #[test]
    fn edge_endpoint_qa_metadata_exposes_screen_position_and_unclipped_rect() {
        let connection_id = Uuid::from_u128(0xED6E);
        let position = egui::pos2(321.5, 654.25);
        let rect = egui::Rect::from_center_size(position, egui::vec2(18.0, 18.0));
        let metadata = edge_endpoint_qa_metadata(connection_id, "source", position, rect);
        assert_eq!(metadata["action"], "reconnect");
        assert_eq!(metadata["connection_id"], connection_id.to_string());
        assert_eq!(metadata["endpoint"], "source");
        assert_eq!(metadata["position"]["x"], position.x);
        assert_eq!(metadata["position"]["y"], position.y);
        assert_eq!(metadata["unclipped_rect"]["min_x"], rect.min.x);
        assert_eq!(metadata["unclipped_rect"]["min_y"], rect.min.y);
        assert_eq!(metadata["unclipped_rect"]["max_x"], rect.max.x);
        assert_eq!(metadata["unclipped_rect"]["max_y"], rect.max.y);
    }

    #[test]
    fn extreme_zoom_disables_precision_hits_without_expanding_node_hit_area() {
        assert!(!node_editor_port_interactions_enabled(
            NODE_EDITOR_MIN_SCALE
        ));
        assert!(!node_editor_resize_interactions_enabled(
            NODE_EDITOR_MIN_SCALE
        ));
        assert!(node_editor_resize_interactions_enabled(
            NODE_EDITOR_RESIZE_INTERACTION_SCALE
        ));
        assert!(!node_editor_port_interactions_enabled(
            NODE_EDITOR_RESIZE_INTERACTION_SCALE
        ));
        assert!(node_editor_port_interactions_enabled(
            NODE_EDITOR_DETAIL_SCALE
        ));

        let graph_node = egui::Rect::from_min_size(
            egui::pos2(250_000.0, 80_000.0),
            egui::vec2(NODE_HEADER_WIDTH, 100.0),
        );
        let scale = NODE_EDITOR_MIN_SCALE;
        let desired_center = egui::pos2(500.0, 400.0);
        let translation = desired_center.to_vec2() - graph_node.center().to_vec2() * scale;
        let to_global = egui::emath::TSTransform::new(translation, scale);
        let screen_node = to_global * graph_node;
        assert!(screen_node.width() < 2.0);
        assert!(screen_node.height() < 1.0);

        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let exclusions = [graph_node];
        let mut state = None;
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(screen_node.center()),
            canvas,
            &exclusions,
            to_global,
            1.0,
        );
        assert!(
            state.is_none(),
            "the actual tiny node still owns its pixels"
        );

        let nearby_empty_screen = screen_node.center() + egui::vec2(4.0, 0.0);
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(nearby_empty_screen),
            canvas,
            &exclusions,
            to_global,
            2.0,
        );
        assert_eq!(
            state
                .expect("nearby overview space remains canvas")
                .position,
            nearby_empty_screen
        );

        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
        let pin = QaPin {
            info: pin_info(PortDataType::Image, false),
            component_id: "node_editor.port.extreme_zoom_test".to_string(),
            to_global,
            graph_center: Some(graph_node.center()),
            address: None,
            direction: PortDirection::Input,
            connected: false,
            canvas_clip: canvas,
            rendered_ports,
        };
        let pin_rect = pin.pin_rect(0.0, 0.0, 20.0, 13.0);
        assert!(
            !pin_rect.is_positive(),
            "overview sockets cannot steal a drag"
        );

        // At this scale every fixed-width screen resize region overlaps this
        // tiny container. The resize dispatcher therefore gates all of them
        // with the same precision-interaction threshold.
        let tiny_container = egui::Rect::from_center_size(
            desired_center,
            egui::vec2(MIN_CONTAINER_SIZE.x * scale, MIN_CONTAINER_SIZE.y * scale),
        );
        assert!(resize_regions(tiny_container)
            .iter()
            .any(|(_, _, rect, _)| rect.contains(desired_center)));
        assert!(!node_editor_resize_interactions_enabled(scale));
    }

    #[test]
    fn graph_is_derived_from_three_container_levels_and_canonical_connections() {
        let (project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        let (snarl, containers) = build_snarl(&project, composition_id);
        let items = snarl.nodes().copied().collect::<Vec<_>>();

        assert_eq!(containers.len(), 3);
        for owner in [
            PortOwner::Composition(composition_id),
            PortOwner::Track(track_id),
            PortOwner::Clip(clip_id),
        ] {
            assert!(items.contains(&GraphItem::Container(owner)));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::ExternalInputs,
            }));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::InternalMetadata,
            }));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::ImageSink,
            }));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::ExternalImage,
            }));
            assert_eq!(
                input_definitions(
                    &project,
                    GraphItem::PortAnchor {
                        owner,
                        kind: PortAnchorKind::ExternalInputs,
                    },
                )
                .len(),
                3
            );
        }
        assert!(items.contains(&GraphItem::Node(solid_id)));
        assert!(items.contains(&GraphItem::Node(merge_id)));
        assert!(snarl.wires().count() >= 2);

        let merge_inputs = input_definitions(&project, GraphItem::Node(merge_id));
        assert!(merge_inputs.iter().any(|pin| pin.key == MERGE_IMAGES_PORT));
        let metadata = output_definitions(
            &project,
            GraphItem::PortAnchor {
                owner: PortOwner::Clip(clip_id),
                kind: PortAnchorKind::InternalMetadata,
            },
        );
        assert_eq!(metadata[0].key, TIME_PORT);
        assert_eq!(metadata[1].key, FRAME_PORT);
    }

    #[test]
    fn transformed_container_chrome_exclusively_owns_secondary_click_but_empty_body_does_not() {
        let (project, composition_id, track_id, _, _, _) = fixture();
        let (rects, exclusion_rects, rendered_to_global, containers) =
            render_test_graph_with_context_menu_exclusions(&project, composition_id);
        let header_screen_rect = rects
            .get(&format!(
                "node_editor.container_header.{}",
                qa_container_key(PortOwner::Track(track_id))
            ))
            .copied()
            .expect("rendered Track header");
        let header_graph_position = rendered_to_global.inverse() * header_screen_rect.center();
        assert!(
            exclusion_rects
                .iter()
                .any(|rect| rect.contains(header_graph_position)),
            "the rendered Track header must be claimed by a Snarl hit rect"
        );

        let track_visual = containers
            .iter()
            .find(|container| container.owner == PortOwner::Track(track_id))
            .expect("Track visual");
        let track_rect = track_visual.rect();
        let empty_body_graph_position =
            egui::pos2(track_rect.right() - 40.0, track_rect.bottom() - 40.0);
        let track_body = egui::Rect::from_min_max(
            egui::pos2(
                track_rect.left(),
                track_rect.top() + CONTAINER_HEADER_HEIGHT,
            ),
            track_rect.max,
        );
        assert!(track_body.contains(empty_body_graph_position));
        assert!(
            !exclusion_rects
                .iter()
                .any(|rect| rect.contains(empty_body_graph_position)),
            "empty container body must not be claimed by Snarl chrome"
        );

        // Exercise the same inverse-transform path as a panned and zoomed
        // Node Editor canvas, rather than comparing screen-space rectangles.
        let to_global = egui::emath::TSTransform::new(egui::vec2(347.0, -73.0), 0.72);
        let canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let header_screen_position = to_global * header_graph_position;
        let empty_body_screen_position = to_global * empty_body_graph_position;
        assert!(canvas_rect.contains(header_screen_position));
        assert!(canvas_rect.contains(empty_body_screen_position));

        let mut state = None;
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(header_screen_position),
            canvas_rect,
            &exclusion_rects,
            to_global,
            1.0,
        );
        assert!(
            state.is_none(),
            "Track header click must not open global Create menu"
        );

        state = Some(ContextMenuState::new(egui::pos2(1.0, 1.0), 0.0));
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(header_screen_position),
            canvas_rect,
            &exclusion_rects,
            to_global,
            2.0,
        );
        assert!(
            state.is_none(),
            "Track header click must close a stale global Create menu"
        );

        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(empty_body_screen_position),
            canvas_rect,
            &exclusion_rects,
            to_global,
            3.0,
        );
        let context = state.expect("empty Track body should open global Create menu");
        assert_eq!(context.position, empty_body_screen_position);
    }

    #[test]
    fn rendered_edges_include_explicit_output_binding_and_derived_dependencies() {
        let (mut project, composition_id, track_id, clip_id, _solid_id, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let rects = render_test_graph(&project, composition_id);

        for connection in &project.connections {
            let id = format!("node_editor.edge:{}", connection.id);
            let rect = rects
                .get(&id)
                .unwrap_or_else(|| panic!("missing rendered explicit edge {id}"));
            assert!(rect.is_positive(), "explicit edge has empty bbox: {rect:?}");
        }

        let binding = format!(
            "node_editor.edge.output_binding:{}:{merge_id}",
            qa_container_key(PortOwner::Clip(clip_id))
        );
        assert!(
            rects.get(&binding).is_some_and(egui::Rect::is_positive),
            "missing Clip output binding edge"
        );

        let track_dependency = format!(
            "node_editor.edge.derived:{}:{}",
            qa_container_key(PortOwner::Track(track_id)),
            qa_container_key(PortOwner::Clip(clip_id))
        );
        assert!(
            rects
                .get(&track_dependency)
                .is_some_and(egui::Rect::is_positive),
            "missing evaluator-derived Clip → Track edge"
        );
    }

    #[test]
    fn edge_and_corner_resize_geometry_is_absolute_and_preserves_children() {
        let owner = PortOwner::Clip(Uuid::from_u128(0x99));
        let base = ContainerResizeState {
            owner,
            edge: ContainerResizeEdge::Right,
            start_pointer: egui::pos2(0.0, 0.0),
            start_position: [100.0, 120.0],
            start_size: [500.0, 300.0],
        };
        let (position, size) = resized_container_geometry(&base, egui::vec2(40.0, 70.0), None);
        assert_eq!(position, [100.0, 120.0]);
        assert_eq!(size, [540.0, 300.0]);

        let corner = ContainerResizeState {
            edge: ContainerResizeEdge::BottomRight,
            ..base
        };
        let (position, size) = resized_container_geometry(&corner, egui::vec2(40.0, 70.0), None);
        assert_eq!(position, [100.0, 120.0]);
        assert_eq!(size, [540.0, 370.0]);

        let children = egui::Rect::from_min_max(egui::pos2(180.0, 230.0), egui::pos2(650.0, 460.0));
        let shrinking = ContainerResizeState {
            edge: ContainerResizeEdge::BottomRight,
            ..base
        };
        let (position, size) =
            resized_container_geometry(&shrinking, egui::vec2(-400.0, -300.0), Some(children));
        let result = container_rect(position, size);
        assert!(result.right() >= children.right() + AUTO_LAYOUT_NODE_PADDING - 0.01);
        assert!(result.bottom() >= children.bottom() + AUTO_LAYOUT_NODE_PADDING - 0.01);

        let regions = resize_regions(egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(500.0, 300.0),
        ));
        assert_eq!(regions.len(), 8);
        assert!(regions.iter().all(|(_, _, rect, _)| rect.is_positive()));
    }

    #[test]
    fn clip_activity_and_disabled_state_have_distinct_inactive_reasons() {
        let (mut project, _, _, clip_id, solid_id, _) = fixture();
        let clip = project.get_clip(clip_id).unwrap();

        assert!(!clip_is_active(clip, 0.999));
        assert!(clip_is_active(clip, 1.0));
        assert!(clip_is_active(clip, 5.999));
        assert!(!clip_is_active(clip, 6.0));
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 0.5),
            Some(GraphItemInactiveReason::OutsideClipRange)
        );
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 1.0),
            None
        );

        project.get_node_mut(solid_id).unwrap().enabled = false;
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 1.0),
            Some(GraphItemInactiveReason::Disabled)
        );
        // Disabled is the primary authored reason even when the Clip is also
        // outside its half-open active range.
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 0.5),
            Some(GraphItemInactiveReason::Disabled)
        );
        assert!(graph_item_inactive(
            &project,
            GraphItem::Node(solid_id),
            0.5
        ));
    }

    #[test]
    fn derived_wire_secondary_hit_is_display_only_instead_of_blank_canvas() {
        let derived = RenderedEdge {
            kind: RenderedEdgeKind::DerivedOutput {
                owner: PortOwner::Track(Uuid::from_u128(0xD001)),
                source: PortOwner::Clip(Uuid::from_u128(0xD002)),
            },
            start: egui::pos2(100.0, 180.0),
            control_a: egui::pos2(180.0, 180.0),
            control_b: egui::pos2(320.0, 180.0),
            end: egui::pos2(400.0, 180.0),
        };
        let hit_point = egui::pos2(250.0, 180.0);

        assert_eq!(
            wire_secondary_click_hit(&[derived], hit_point),
            Some(WireSecondaryClickHit::DisplayOnly)
        );
        assert_eq!(
            wire_secondary_click_hit(&[], hit_point),
            None,
            "blank canvas must remain distinguishable from a display-only wire"
        );
    }

    #[test]
    fn wire_knife_detects_midspan_intersection_of_long_segments() {
        let knife_start = egui::pos2(10.0, -1_000.0);
        let knife_end = egui::pos2(10.0, 1_000.0);
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: Uuid::new_v4(),
            },
            start: egui::pos2(-1_000.0, 0.0),
            control_a: egui::pos2(-333.333_34, 0.0),
            control_b: egui::pos2(333.333_34, 0.0),
            end: egui::pos2(1_000.0, 0.0),
        };

        assert!(segments_intersect(
            knife_start,
            knife_end,
            edge.start,
            edge.end,
        ));
        assert!(knife_segment_hits_edge(knife_start, knife_end, &edge));
    }

    #[test]
    fn alt_drag_knife_batches_explicit_and_output_binding_but_preserves_derived_wires(
    ) -> Result<(), String> {
        let (mut project, _, track_id, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .map_err(|error| error.to_string())?;
        let connection_ids = project
            .connections
            .iter()
            .map(|connection| connection.id)
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(connection_ids.len(), 2);
        let edges = vec![
            RenderedEdge {
                kind: RenderedEdgeKind::ProjectConnection {
                    connection_id: connection_ids[0],
                },
                start: egui::pos2(100.0, 160.0),
                control_a: egui::pos2(180.0, 120.0),
                control_b: egui::pos2(320.0, 200.0),
                end: egui::pos2(400.0, 160.0),
            },
            RenderedEdge {
                kind: RenderedEdgeKind::ProjectConnection {
                    connection_id: connection_ids[1],
                },
                start: egui::pos2(100.0, 230.0),
                control_a: egui::pos2(180.0, 190.0),
                control_b: egui::pos2(320.0, 270.0),
                end: egui::pos2(400.0, 230.0),
            },
            RenderedEdge {
                kind: RenderedEdgeKind::OutputBinding {
                    owner: PortOwner::Clip(clip_id),
                    node_id: merge_id,
                },
                start: egui::pos2(100.0, 300.0),
                control_a: egui::pos2(180.0, 260.0),
                control_b: egui::pos2(320.0, 340.0),
                end: egui::pos2(400.0, 300.0),
            },
            RenderedEdge {
                kind: RenderedEdgeKind::DerivedOutput {
                    owner: PortOwner::Track(track_id),
                    source: PortOwner::Clip(clip_id),
                },
                start: egui::pos2(100.0, 350.0),
                control_a: egui::pos2(180.0, 310.0),
                control_b: egui::pos2(320.0, 390.0),
                end: egui::pos2(400.0, 350.0),
            },
        ];
        assert!(knife_segment_hits_edge(
            egui::pos2(250.0, 100.0),
            egui::pos2(250.0, 290.0),
            &edges[0],
        ));
        assert!(!knife_segment_hits_edge(
            egui::pos2(460.0, 100.0),
            egui::pos2(460.0, 290.0),
            &edges[0],
        ));

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let start = egui::pos2(250.0, 100.0);
        let end = egui::pos2(250.0, 390.0);
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let frames = vec![
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: alt,
            }],
            vec![egui::Event::PointerMoved(egui::pos2(250.0, 245.0))],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: alt,
            }],
        ];
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut queued = Vec::new();
        for (frame, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers: alt,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: &edges,
                                rendered_ports: &ports,
                                canvas_clip: screen,
                                graph_item_rects: &[],
                                to_global: egui::emath::TSTransform::IDENTITY,
                            },
                        ));
                    });
                },
            ));
        }
        let [QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires { wires: crossed })] =
            queued.as_slice()
        else {
            return Err(format!("knife did not emit one batch: {queued:?}"));
        };
        let mut expected = connection_ids
            .iter()
            .copied()
            .map(|connection_id| NodeEditorEditableWire::ProjectConnection { connection_id })
            .chain(std::iter::once(NodeEditorEditableWire::OutputBinding {
                owner: PortOwner::Clip(clip_id),
                node_id: merge_id,
            }))
            .collect::<Vec<_>>();
        expected.sort_by_key(|target| editable_wire_sort_key(*target));
        assert_eq!(crossed, &expected);
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            queued,
            &mut history,
            &mut state,
        ));
        assert!(project
            .connections
            .iter()
            .all(|connection| !connection_ids.contains(&connection.id)));
        let edited_clip = project
            .get_clip(clip_id)
            .ok_or_else(|| "knife removed its Clip".to_string())?;
        assert_eq!(edited_clip.output_node_id, None);
        assert_eq!(project.find_track_for_clip(clip_id), Some(track_id));
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
        Ok(())
    }

    #[test]
    fn zero_hit_knife_stroke_emits_no_edit_and_keeps_history_clean() {
        let (project, _, _, _, solid_id, merge_id) = fixture();
        let connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap()
            .id;
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection { connection_id },
            start: egui::pos2(100.0, 180.0),
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(400.0, 180.0),
        };
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let start = egui::pos2(500.0, 80.0);
        let end = egui::pos2(560.0, 120.0);
        let frames = vec![
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: alt,
            }],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: alt,
            }],
        ];
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut queued = Vec::new();
        for (frame, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers: alt,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: std::slice::from_ref(&edge),
                                rendered_ports: &ports,
                                canvas_clip: screen,
                                graph_item_rects: &[],
                                to_global: egui::emath::TSTransform::IDENTITY,
                            },
                        ));
                    });
                },
            ));
        }
        assert!(queued.is_empty());
        assert!(state.wire_knife.is_none());
    }

    #[test]
    fn moving_composition_moves_track_clip_and_nodes_without_changing_containment() {
        let (mut project, composition_id, track_id, clip_id, solid_id, _) = fixture();
        let original_clip_ids = project.get_track(track_id).unwrap().clip_ids.clone();
        let original_node_ids = project.get_clip(clip_id).unwrap().node_ids.clone();

        assert!(translate_container(
            &mut project,
            PortOwner::Composition(composition_id),
            [25.0, -15.0]
        ));

        assert_eq!(
            project.get_track(track_id).unwrap().ui_position,
            [135.0, 125.0]
        );
        assert_eq!(
            project.get_clip(clip_id).unwrap().ui_position,
            [285.0, 245.0]
        );
        assert_eq!(
            project.get_node(solid_id).unwrap().ui_position,
            [475.0, 375.0]
        );
        assert_eq!(
            project.get_track(track_id).unwrap().clip_ids,
            original_clip_ids
        );
        assert_eq!(
            project.get_clip(clip_id).unwrap().node_ids,
            original_node_ids
        );
        assert_eq!(project.connections.len(), 2);
    }

    #[test]
    fn clip_resize_and_timing_edits_persist_on_the_clip_only() {
        let (mut project, _, _, clip_id, solid_id, _) = fixture();
        assert!(set_container_size(
            &mut project,
            PortOwner::Clip(clip_id),
            [720.0, 520.0]
        ));
        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Clip(clip_id),
                key: "start_time".into(),
                time: 0.0,
                value: PropertyValue::Number(OrderedFloat(2.5)),
            }
        ));
        assert_eq!(project.get_clip(clip_id).unwrap().ui_size, [720.0, 520.0]);
        assert_eq!(
            project.get_clip(clip_id).unwrap().start_time.into_inner(),
            2.5
        );
        assert!(project.get_node(solid_id).is_some());
    }

    #[test]
    fn clip_owned_node_properties_use_local_time_for_evaluation_and_keyframe_edits() {
        let (mut project, composition_id, _, clip_id, solid_id, _) = fixture();
        {
            let clip = project.get_clip_mut(clip_id).unwrap();
            clip.start_time = OrderedFloat(4.0);
            clip.duration = OrderedFloat(10.0);
            clip.trim_in = OrderedFloat(1.25);
            clip.time_stretch = OrderedFloat(1.5);
        }
        let animated = Property::keyframe(vec![
            Keyframe::new(
                0.0,
                PropertyValue::Number(OrderedFloat(0.0)),
                EasingFunction::Linear,
            ),
            Keyframe::new(
                10.0,
                PropertyValue::Number(OrderedFloat(100.0)),
                EasingFunction::Linear,
            ),
        ]);
        project
            .get_node_mut(solid_id)
            .unwrap()
            .set_property("opacity".to_string(), animated.clone())
            .expect("solid factory initializes opacity");

        let global_time = 6.0;
        let inspector_and_renderer_time =
            project.get_clip(clip_id).unwrap().local_time(global_time);
        assert_eq!(inspector_and_renderer_time, 4.25);
        assert_eq!(
            node_property_time(&project, solid_id, global_time),
            inspector_and_renderer_time
        );
        assert_eq!(
            project
                .get_node(solid_id)
                .unwrap()
                .properties()
                .get("opacity")
                .unwrap()
                .evaluate_at(node_property_time(&project, solid_id, global_time))
                .unwrap(),
            PropertyValue::Number(OrderedFloat(42.5))
        );

        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Node(solid_id),
                key: "opacity".into(),
                time: inspector_and_renderer_time,
                value: PropertyValue::Number(OrderedFloat(91.0)),
            }
        ));
        let clip_node_property = project
            .get_node(solid_id)
            .unwrap()
            .properties()
            .get("opacity")
            .unwrap();
        assert_eq!(
            clip_node_property
                .evaluate_at(inspector_and_renderer_time)
                .unwrap(),
            PropertyValue::Number(OrderedFloat(91.0))
        );
        assert!(clip_node_property.has_keyframe_at(inspector_and_renderer_time, 0.001));
        assert!(!clip_node_property.has_keyframe_at(global_time, 0.001));

        let root_id = Uuid::from_u128(0x9_101);
        let mut root = generator_node(
            "Root",
            GeneratorNodeRequest::Solid {
                color: Color::default(),
            },
        );
        root.id = root_id;
        root.set_property("opacity".to_string(), animated)
            .expect("solid factory initializes opacity");
        project.add_node(root);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), root_id)
            .unwrap();
        assert_eq!(
            node_property_time(&project, root_id, global_time),
            global_time,
            "Composition-owned Node time stays in the global domain"
        );
        let root_property_time = node_property_time(&project, root_id, global_time);
        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Node(root_id),
                key: "opacity".into(),
                time: root_property_time,
                value: PropertyValue::Number(OrderedFloat(55.0)),
            }
        ));
        let root_property = project
            .get_node(root_id)
            .unwrap()
            .properties()
            .get("opacity")
            .unwrap();
        assert!(root_property.has_keyframe_at(global_time, 0.001));
        assert_eq!(
            root_property.evaluate_at(global_time).unwrap(),
            PropertyValue::Number(OrderedFloat(55.0))
        );
    }

    #[test]
    fn numeric_drag_text_typing_and_color_popup_each_commit_one_undoable_gesture() {
        let (mut numeric_project, _, _, clip_id, solid_id, _) = fixture();
        {
            let clip = numeric_project.get_clip_mut(clip_id).unwrap();
            clip.start_time = OrderedFloat(4.0);
            clip.trim_in = OrderedFloat(1.25);
            clip.time_stretch = OrderedFloat(1.5);
        }
        let numeric_initial = numeric_project.clone();
        let numeric_time = node_property_time(&numeric_project, solid_id, 6.0);
        assert_eq!(numeric_time, 4.25);
        let mut numeric_history = HistoryManager::new();
        numeric_history.push_project_state(numeric_initial.clone());
        let mut numeric_state = NodeEditorState::default();
        for value in [10.0, 20.0, 30.0] {
            assert!(apply_queued_node_edits(
                &mut numeric_project,
                vec![queued_property_edit(
                    PortOwner::Node(solid_id),
                    "opacity",
                    numeric_time,
                    PropertyValue::Number(OrderedFloat(value)),
                    false,
                )],
                &mut numeric_history,
                &mut numeric_state,
            ));
            assert_eq!(numeric_history.undo_depth(), 1);
        }
        assert!(!apply_queued_node_edits(
            &mut numeric_project,
            vec![queued_finish(PortOwner::Node(solid_id), "opacity")],
            &mut numeric_history,
            &mut numeric_state,
        ));
        assert!(numeric_state.pending_continuous_edit.is_none());
        let numeric_edited = numeric_project.clone();
        assert_single_gesture_undo_redo(&mut numeric_history, &numeric_initial, &numeric_edited);

        let (mut text_project, _, _, _, text_node_id, _) = fixture();
        let text_initial = text_project.clone();
        let mut text_history = HistoryManager::new();
        text_history.push_project_state(text_initial.clone());
        let mut text_state = NodeEditorState::default();
        for name in ["S", "So", "Solid renamed"] {
            assert!(apply_queued_node_edits(
                &mut text_project,
                vec![QueuedNodeEdit::Continuous {
                    pending: NodeEditorPendingEdit {
                        owner: PortOwner::Node(text_node_id),
                        key: "$name".into(),
                    },
                    edit: Some(NodeEdit::Rename {
                        node_id: text_node_id,
                        name: name.into(),
                    }),
                    finished: false,
                }],
                &mut text_history,
                &mut text_state,
            ));
            assert_eq!(text_history.undo_depth(), 1);
        }
        apply_queued_node_edits(
            &mut text_project,
            vec![queued_finish(PortOwner::Node(text_node_id), "$name")],
            &mut text_history,
            &mut text_state,
        );
        let text_edited = text_project.clone();
        assert_single_gesture_undo_redo(&mut text_history, &text_initial, &text_edited);

        let (mut color_project, _, _, _, color_node_id, _) = fixture();
        let color_initial = color_project.clone();
        let mut color_history = HistoryManager::new();
        color_history.push_project_state(color_initial.clone());
        let mut color_state = NodeEditorState::default();
        for color in [
            library::model::frame::color::Color {
                r: 20,
                g: 30,
                b: 40,
                a: 255,
            },
            library::model::frame::color::Color {
                r: 80,
                g: 90,
                b: 100,
                a: 220,
            },
        ] {
            assert!(apply_queued_node_edits(
                &mut color_project,
                vec![queued_property_edit(
                    PortOwner::Node(color_node_id),
                    "color",
                    0.0,
                    PropertyValue::Color(color),
                    false,
                )],
                &mut color_history,
                &mut color_state,
            ));
            assert_eq!(color_history.undo_depth(), 1);
        }
        apply_queued_node_edits(
            &mut color_project,
            vec![queued_finish(PortOwner::Node(color_node_id), "color")],
            &mut color_history,
            &mut color_state,
        );
        let color_edited = color_project.clone();
        assert_single_gesture_undo_redo(&mut color_history, &color_initial, &color_edited);
    }

    #[test]
    fn owner_or_control_switch_flushes_previous_edit_and_atomic_checkbox_commits_immediately() {
        let (mut project, _, _, _, node_id, _) = fixture();
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial);
        let mut state = NodeEditorState::default();

        apply_queued_node_edits(
            &mut project,
            vec![queued_property_edit(
                PortOwner::Node(node_id),
                "opacity",
                0.0,
                PropertyValue::Number(OrderedFloat(25.0)),
                false,
            )],
            &mut history,
            &mut state,
        );
        let after_numeric = project.clone();
        assert_eq!(history.undo_depth(), 1);

        apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Continuous {
                pending: NodeEditorPendingEdit {
                    owner: PortOwner::Node(node_id),
                    key: "$name".into(),
                },
                edit: Some(NodeEdit::Rename {
                    node_id,
                    name: "switched control".into(),
                }),
                finished: false,
            }],
            &mut history,
            &mut state,
        );
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&project), Some(after_numeric.clone()));
        assert_eq!(history.redo(&after_numeric), Some(project.clone()));

        let before_owner_switch = project.clone();
        let project_lock = Arc::new(RwLock::new(project));
        assert!(flush_pending_continuous_edit(
            &project_lock,
            &mut history,
            &mut state,
        ));
        assert_eq!(history.undo_depth(), 3);
        assert!(state.pending_continuous_edit.is_none());
        assert_eq!(
            history.undo(&before_owner_switch),
            Some(after_numeric.clone())
        );
        assert_eq!(
            history.redo(&after_numeric),
            Some(before_owner_switch.clone())
        );

        let mut project = project_lock.read().unwrap().clone();
        apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
                node_id,
                enabled: false,
            })],
            &mut history,
            &mut state,
        );
        assert_eq!(history.undo_depth(), 4);
        assert!(state.pending_continuous_edit.is_none());
    }

    #[test]
    fn zero_time_stretch_is_preserved_as_freeze_and_negative_input_is_rejected() {
        let (mut project, _, _, clip_id, _, _) = fixture();
        {
            let clip = project.get_clip_mut(clip_id).unwrap();
            clip.trim_in = OrderedFloat(2.25);
            clip.time_stretch = OrderedFloat(0.0);
        }
        let serialized = serde_json::to_string(&project).unwrap();
        let mut loaded: Project = serde_json::from_str(&serialized).unwrap();
        let loaded_value = loaded.get_clip(clip_id).unwrap().time_stretch.into_inner();
        assert_eq!(
            Clip::validate_timing_property_value(
                "time_stretch",
                &PropertyValue::Number(OrderedFloat(loaded_value)),
            )
            .unwrap(),
            0.0
        );
        assert!(Clip::validate_timing_property_value(
            "time_stretch",
            &PropertyValue::Number(OrderedFloat(-0.5)),
        )
        .is_err());

        assert!(apply_edit(
            &mut loaded,
            NodeEdit::SetProperty {
                owner: PortOwner::Clip(clip_id),
                key: "time_stretch".into(),
                time: 9.0,
                value: PropertyValue::Number(OrderedFloat(loaded_value)),
            }
        ));
        let clip = loaded.get_clip(clip_id).unwrap();
        assert_eq!(clip.time_stretch, OrderedFloat(0.0));
        assert_eq!(clip.local_time(clip.start_time.into_inner()), 2.25);
        assert_eq!(clip.local_time(clip.start_time.into_inner() + 100.0), 2.25);
    }

    #[test]
    fn deleting_a_clip_removes_only_its_owned_leaf_nodes() {
        let (mut project, _, track_id, clip_id, solid_id, merge_id) = fixture();
        assert!(apply_edit(
            &mut project,
            NodeEdit::Delete {
                owner: PortOwner::Clip(clip_id),
            }
        ));
        assert!(project.get_clip(clip_id).is_none());
        assert!(project.get_node(solid_id).is_none());
        assert!(project.get_node(merge_id).is_none());
        assert!(project.get_track(track_id).is_some());
        assert!(project.connections.is_empty());
    }

    #[test]
    fn scc_ranking_is_deterministic_and_keeps_cycles_in_one_rank() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);
        let nodes = vec![c, b, a];
        let edges = vec![(a, b), (b, a), (b, c)];
        let ranks = rank_nodes_by_scc(&nodes, &edges);
        assert_eq!(ranks[&a], ranks[&b]);
        assert!(ranks[&b] < ranks[&c]);
    }

    #[test]
    fn full_layout_preserves_hierarchy_non_overlap_and_global_ltr_in_rendered_rects() {
        let (mut project, ids) = adversarial_hierarchy_fixture();
        assert!(layout_needs_reflow(&project, ids.composition));

        let full = compute_full_composition_layout(&project, ids.composition).unwrap();
        for scope in [
            AutoLayoutScope::All,
            AutoLayoutScope::Container(PortOwner::Composition(ids.composition)),
        ] {
            assert_eq!(
                compute_auto_layout(&project, ids.composition, scope).unwrap(),
                full
            );
        }

        assert!(apply_auto_layout(&mut project, ids.composition, &full));
        assert!(!layout_needs_reflow(&project, ids.composition));

        let rects = render_test_graph(&project, ids.composition);
        let composition = project.get_composition(ids.composition).unwrap();
        let composition_rect = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Composition(ids.composition))
            ))
            .copied()
            .unwrap();
        let scale = composition_rect.width() / composition.ui_size[0];
        let screen_padding = AUTO_LAYOUT_NODE_PADDING * scale;

        let node_ids = [
            ids.solid,
            ids.merge,
            ids.text,
            ids.text_fill,
            ids.shape,
            ids.shape_fill,
            ids.composition_node,
        ];
        let rendered_nodes = node_ids
            .iter()
            .map(|node_id| {
                let rect = rects
                    .get(&format!("node_editor.node:{node_id}"))
                    .copied()
                    .unwrap();
                let estimated = estimated_node_size(&project, *node_id);
                assert!(
                    rect.width() <= estimated.x * scale + 1.0,
                    "{} ({node_id}): {rect:?}, estimated={estimated:?}, scale={scale}",
                    node_title(&project, *node_id),
                );
                assert!(
                    rect.height() <= estimated.y * scale + 1.0,
                    "{} ({node_id}): {rect:?}, estimated={estimated:?}, scale={scale}",
                    node_title(&project, *node_id),
                );
                (*node_id, rect)
            })
            .collect::<HashMap<_, _>>();

        for (index, left_id) in node_ids.iter().enumerate() {
            for right_id in &node_ids[index + 1..] {
                let left = rendered_nodes[left_id].expand(screen_padding * 0.5);
                let right = rendered_nodes[right_id].expand(screen_padding * 0.5);
                assert!(
                    !left.intersects(right),
                    "rendered Nodes overlap: {left_id} {left:?}, {right_id} {right:?}"
                );
            }
        }

        let immediate_owners = [
            (ids.solid, PortOwner::Clip(ids.clip)),
            (ids.merge, PortOwner::Clip(ids.clip)),
            (ids.text, PortOwner::Clip(ids.sibling_clip)),
            (ids.text_fill, PortOwner::Clip(ids.sibling_clip)),
            (ids.shape, PortOwner::Track(ids.track)),
            (ids.shape_fill, PortOwner::Track(ids.track)),
            (
                ids.composition_node,
                PortOwner::Composition(ids.composition),
            ),
        ];
        for (node_id, owner) in immediate_owners {
            let owner_key = qa_container_key(owner);
            let owner_rect = rects
                .get(&format!("node_editor.container.{owner_key}"))
                .copied()
                .unwrap();
            let node_rect = rendered_nodes[&node_id];
            assert!(
                rect_contains_rect(owner_rect.expand(0.5), node_rect),
                "{node_id} escaped immediate owner {owner_key}: {node_rect:?} not in {owner_rect:?}"
            );
            assert!(
                node_rect.top() >= owner_rect.top() + CONTAINER_HEADER_HEIGHT * scale - 1.0,
                "{node_id} overlaps integrated header of {owner_key}"
            );
        }

        let sibling_clips = [ids.clip, ids.sibling_clip, ids.empty_clip];
        for (index, left_id) in sibling_clips.iter().enumerate() {
            let left = rects
                .get(&format!(
                    "node_editor.container.{}",
                    qa_container_key(PortOwner::Clip(*left_id))
                ))
                .copied()
                .unwrap()
                .expand(screen_padding * 0.5);
            for right_id in &sibling_clips[index + 1..] {
                let right = rects
                    .get(&format!(
                        "node_editor.container.{}",
                        qa_container_key(PortOwner::Clip(*right_id))
                    ))
                    .copied()
                    .unwrap()
                    .expand(screen_padding * 0.5);
                assert!(
                    !left.intersects(right),
                    "Clip overlap: {left:?} / {right:?}"
                );
            }
        }

        let track = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Track(ids.track))
            ))
            .copied()
            .unwrap();
        let empty_track = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Track(ids.empty_track))
            ))
            .copied()
            .unwrap();
        assert!(!track.intersects(empty_track));
        assert!(rect_contains_rect(composition_rect.expand(0.5), track));
        assert!(rect_contains_rect(
            composition_rect.expand(0.5),
            empty_track
        ));

        let all_nodes = composition_graph_node_ids(&project, ids.composition);
        let edges = canonical_edges(&project, &all_nodes);
        let ranks = rank_nodes_by_scc(&all_nodes, &edges);
        for (from, to) in edges {
            if ranks[&from] == ranks[&to] {
                continue;
            }
            let from_rect = rendered_nodes[&from];
            let to_rect = rendered_nodes[&to];
            assert!(
                from_rect.right() + screen_padding <= to_rect.left() + 1.0,
                "canonical edge is not LTR: {from} {from_rect:?} -> {to} {to_rect:?}"
            );
        }
    }

    #[test]
    fn scoped_layouts_leave_every_out_of_scope_entity_byte_identical() {
        let (mut base, ids) = adversarial_hierarchy_fixture();
        let full = compute_full_composition_layout(&base, ids.composition).unwrap();
        apply_auto_layout(&mut base, ids.composition, &full);
        assert!(!layout_needs_reflow(&base, ids.composition));

        let mut track_project = base.clone();
        let unrelated_track_before = track_project.get_track(ids.empty_track).unwrap().clone();
        let root_node_before = track_project
            .get_node(ids.composition_node)
            .unwrap()
            .clone();
        let track_plan = compute_auto_layout(
            &track_project,
            ids.composition,
            AutoLayoutScope::Container(PortOwner::Track(ids.track)),
        )
        .unwrap();
        assert!(!track_plan.track_layouts.contains_key(&ids.empty_track));
        assert!(!track_plan
            .node_positions
            .contains_key(&ids.composition_node));
        apply_auto_layout(&mut track_project, ids.composition, &track_plan);
        assert_eq!(
            track_project.get_track(ids.empty_track).unwrap(),
            &unrelated_track_before
        );
        assert_eq!(
            track_project.get_node(ids.composition_node).unwrap(),
            &root_node_before
        );
        assert!(!layout_needs_reflow(&track_project, ids.composition));

        let mut clip_project = base.clone();
        let sibling_before = clip_project.get_clip(ids.sibling_clip).unwrap().clone();
        let text_before = clip_project.get_node(ids.text).unwrap().clone();
        let shape_before = clip_project.get_node(ids.shape).unwrap().clone();
        let clip_plan = compute_auto_layout(
            &clip_project,
            ids.composition,
            AutoLayoutScope::Container(PortOwner::Clip(ids.clip)),
        )
        .unwrap();
        assert!(!clip_plan.clip_layouts.contains_key(&ids.sibling_clip));
        assert!(!clip_plan.node_positions.contains_key(&ids.text));
        assert!(!clip_plan.node_positions.contains_key(&ids.shape));
        apply_auto_layout(&mut clip_project, ids.composition, &clip_plan);
        assert_eq!(
            clip_project.get_clip(ids.sibling_clip).unwrap(),
            &sibling_before
        );
        assert_eq!(clip_project.get_node(ids.text).unwrap(), &text_before);
        assert_eq!(clip_project.get_node(ids.shape).unwrap(), &shape_before);
        assert!(
            !layout_needs_reflow(&clip_project, ids.composition),
            "Clip-scoped plan violated layout invariants: {clip_plan:#?}"
        );

        let mut selection_project = base.clone();
        let merge_position = selection_project.get_node(ids.merge).unwrap().ui_position;
        selection_project
            .get_node_mut(ids.solid)
            .unwrap()
            .ui_position = merge_position;
        let merge_before = selection_project.get_node(ids.merge).unwrap().clone();
        let text_before = selection_project.get_node(ids.text).unwrap().clone();
        let selection_plan = compute_auto_layout(
            &selection_project,
            ids.composition,
            AutoLayoutScope::Selection(vec![ids.solid]),
        )
        .unwrap();
        assert_eq!(
            selection_plan
                .node_positions
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![ids.solid]
        );
        apply_auto_layout(&mut selection_project, ids.composition, &selection_plan);
        assert_eq!(
            selection_project.get_node(ids.merge).unwrap(),
            &merge_before
        );
        assert_eq!(selection_project.get_node(ids.text).unwrap(), &text_before);
        assert!(!layout_needs_reflow(&selection_project, ids.composition));
    }

    #[test]
    fn new_nodes_use_a_dependency_near_free_slot_inside_clip() {
        let (mut project, _, _, clip_id, existing_id, _) = fixture();
        let node = Node::new_merge("New");
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        let placed = place_node_in_free_slot(
            &mut project,
            node_id,
            NodeContainer::Clip(clip_id),
            egui::Pos2::ZERO,
            &[existing_id],
        )
        .unwrap();
        let existing = project.get_node(existing_id).unwrap();
        assert!(
            placed.x >= existing.ui_position[0] + estimated_node_width() + AUTO_LAYOUT_COLUMN_GAP
        );
        let placed_rect = egui::Rect::from_min_size(placed, estimated_node_size(&project, node_id));
        let clip = project.get_clip(clip_id).unwrap();
        let clip_rect = container_rect(clip.ui_position, clip.ui_size);
        assert!(clip_rect.contains(placed_rect.min));
        assert!(clip_rect.contains(placed_rect.max));
    }

    #[test]
    fn collapsed_reparent_excludes_both_the_header_and_stored_body() {
        let (mut project, composition_id, track_id, clip_id, _, _) = fixture();
        let clip_header = egui::pos2(500.0, 280.0);
        let hidden_clip_body = egui::pos2(500.0, 400.0);

        assert_eq!(
            node_container_at_position(&project, composition_id, clip_header),
            Some(NodeContainer::Clip(clip_id))
        );

        let clip = project.get_clip_mut(clip_id).unwrap();
        clip.ui_collapsed = true;
        assert!(container_rect(clip.ui_position, clip.ui_size).contains(hidden_clip_body));
        let collapsed_geometry = reparent_container_geometries(&project, composition_id)
            .into_iter()
            .find(|geometry| geometry.container == NodeContainer::Clip(clip_id));
        assert!(collapsed_geometry.is_some_and(|geometry| {
            reparent_ineligible_reason(geometry) == Some("collapsed_hidden_content")
        }));

        let node_id = project.get_clip(clip_id).unwrap().node_ids[0];
        // A collapsed owner has no valid content surface. Neither its visible
        // header nor its stored hidden body may become a semantic destination;
        // an explicit expand-on-hover policy can add that later without moving
        // the header or violating exact drop coordinates.
        let node_size = estimated_node_size(&project, node_id);
        let hidden_rect = egui::Rect::from_center_size(hidden_clip_body, node_size);
        assert_eq!(
            deepest_legal_reparent_target(&project, composition_id, hidden_rect, hidden_clip_body,)
                .map(|target| target.container),
            Some(NodeContainer::Track(track_id))
        );
        let header_rect = egui::Rect::from_center_size(clip_header, node_size);
        assert_eq!(
            deepest_legal_reparent_target(&project, composition_id, header_rect, clip_header,)
                .map(|target| target.container),
            Some(NodeContainer::Track(track_id))
        );

        assert!(reparent_test_node_at_drop(
            &mut project,
            composition_id,
            node_id,
            hidden_clip_body,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        project.get_clip_mut(clip_id).unwrap().ui_collapsed = false;
        assert!(project
            .attach_node_to_container(NodeContainer::Track(track_id), node_id)
            .is_ok());
        let expanded_drop = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
            .center()
        });
        assert!(expanded_drop.is_some());
        let Some(expanded_drop) = expanded_drop else {
            return;
        };
        assert!(reparent_test_node_at_drop(
            &mut project,
            composition_id,
            node_id,
            expanded_drop,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );

        project.get_track_mut(track_id).unwrap().ui_collapsed = true;
        let track = project.get_track(track_id).unwrap();
        let track_header = egui::pos2(
            track.ui_position[0] + 24.0,
            track.ui_position[1] + CONTAINER_HEADER_HEIGHT * 0.5,
        );
        let node_size = estimated_node_size(&project, node_id);
        let track_header_rect = egui::Rect::from_center_size(track_header, node_size);
        assert_eq!(
            deepest_legal_reparent_target(
                &project,
                composition_id,
                track_header_rect,
                track_header,
            )
            .map(|target| target.container),
            Some(NodeContainer::Composition(composition_id))
        );
        let hidden_track_body = egui::pos2(track_header.x, track_header.y + 180.0);
        let hidden_track_rect = egui::Rect::from_center_size(hidden_track_body, node_size);
        assert_eq!(
            deepest_legal_reparent_target(
                &project,
                composition_id,
                hidden_track_rect,
                hidden_track_body,
            )
            .map(|target| target.container),
            Some(NodeContainer::Composition(composition_id))
        );
        assert!(project.validate_containment().is_empty());
    }

    #[test]
    fn reparent_overlap_uses_deepest_legal_content_and_authoritative_stacking_order() {
        let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
        let Some(first_clip) = project.get_clip(clip_id).cloned() else {
            assert!(project.get_clip(clip_id).is_some());
            return;
        };
        let mut second_clip = Clip::new("Overlapping Clip", 0.0, 5.0);
        second_clip.ui_position = first_clip.ui_position;
        second_clip.ui_size = first_clip.ui_size;
        let second_clip_id = second_clip.id;
        project.add_clip(second_clip);
        assert!(project
            .attach_clip_to_track(track_id, second_clip_id)
            .is_ok());

        let first_rect = container_rect(first_clip.ui_position, first_clip.ui_size);
        let content = nested_content_rect(first_rect, AUTO_LAYOUT_CLIP_TOP);
        let node_size = estimated_node_size(&project, node_id);
        let node_position = content.center() - node_size * 0.5;
        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let Some(node) = project.get_node_mut(node_id) else {
            assert!(project.get_node(node_id).is_some());
            return;
        };
        node.ui_position = [node_position.x, node_position.y];
        let node_rect = egui::Rect::from_min_size(node_position, node_size);
        let drop_point = node_rect.center();
        let selected =
            deepest_legal_reparent_target(&project, composition_id, node_rect, drop_point);
        assert_eq!(
            selected.map(|target| target.container),
            Some(NodeContainer::Clip(second_clip_id))
        );
        assert!(selected.is_some_and(|target| target.depth == 2 && target.center_inside));
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, node_rect)]),
            &HashMap::from([(node_id, [node_position.x, node_position.y])]),
            drop_point,
            1.0,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Clip(second_clip_id)
        }));

        let Some(second_clip) = project.get_clip_mut(second_clip_id) else {
            assert!(project.get_clip(second_clip_id).is_some());
            return;
        };
        second_clip.ui_collapsed = true;
        assert_eq!(
            deepest_legal_reparent_target(&project, composition_id, node_rect, drop_point,)
                .map(|target| target.container),
            Some(NodeContainer::Clip(clip_id))
        );
    }

    #[test]
    fn reparent_intent_keeps_origin_across_header_padding_until_node_fully_exits() {
        let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        let exact_node_size = egui::vec2(120.0, 60.0);
        let origin_position = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
            .min + egui::vec2(32.0, 32.0)
        });
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [origin_position.x, origin_position.y];
        }
        let Some(origin_node) = project.get_node(node_id) else {
            assert!(project.get_node(node_id).is_some());
            return;
        };
        let origin = NodeEditorNodeDragOrigin {
            container: NodeContainer::Clip(clip_id),
            position: origin_node.ui_position,
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(node_id, origin)]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let small_min = egui::pos2(origin.position[0] + 4.0, origin.position[1] + 3.0);
        let small_rect = egui::Rect::from_min_size(small_min, exact_node_size);
        assert!(node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, small_rect)]),
            &HashMap::from([(node_id, [small_min.x, small_min.y])]),
            small_rect.center(),
            1.0,
        )
        .is_empty());
        let mut non_node_state = NodeEditorState::default();
        record_node_reparent_origins(
            &project,
            &[
                LayoutEdit::MoveContainer {
                    owner: PortOwner::Track(track_id),
                    delta: [48.0, 24.0],
                },
                LayoutEdit::ResizeContainer {
                    owner: PortOwner::Clip(clip_id),
                    position: [200.0, 200.0],
                    size: [800.0, 500.0],
                },
            ],
            &mut non_node_state,
            true,
        );
        assert!(non_node_state.node_reparent.is_none());
        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_rect = container_rect(clip.ui_position, clip.ui_size);
        let padding_min = clip_rect.min + egui::vec2(12.0, 12.0);
        let padding_rect = egui::Rect::from_min_size(padding_min, exact_node_size);
        let padding_positions = HashMap::from([(node_id, [padding_min.x, padding_min.y])]);
        let padding_rects = HashMap::from([(node_id, padding_rect)]);
        let padding_intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &padding_rects,
            &padding_positions,
            padding_rect.center(),
            1.0,
        );
        assert_eq!(padding_intents.len(), 1);
        assert!(padding_intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Clip(clip_id)
                && intent.target.kind == ReparentTargetKind::RetainedOrigin
        }));
        let mut retained_project = project.clone();
        if let Some(node) = retained_project.get_node_mut(node_id) {
            node.ui_position = [padding_min.x, padding_min.y];
        }
        assert!(reparent_nodes_from_intents(
            &mut retained_project,
            &padding_intents,
        ));
        assert_eq!(
            retained_project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let retained_content = retained_project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
        });
        assert!(
            retained_content.is_some_and(|content| { rect_contains_rect(content, padding_rect) })
        );
        assert!(retained_project.validate_containment().is_empty());

        let exited_min = egui::pos2(1_120.0, 470.0);
        let exited_rect = egui::Rect::from_min_size(exited_min, exact_node_size);
        let exited_positions = HashMap::from([(node_id, [exited_min.x, exited_min.y])]);
        let exited_rects = HashMap::from([(node_id, exited_rect)]);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &exited_rects,
            &exited_positions,
            exited_rect.center(),
            1.0,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents
            .first()
            .is_some_and(|intent| intent.target.container == NodeContainer::Track(track_id)));
    }

    #[test]
    fn subthreshold_screen_drag_repairs_containment_without_changing_owner() {
        let (mut project, composition_id, _, clip_id, _, node_id) = fixture();
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        apply_auto_layout(&mut project, composition_id, &plan);
        assert!(!layout_needs_reflow(&project, composition_id));
        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let screen_delta = NODE_REPARENT_DRAG_THRESHOLD - 1.0;
        let graph_delta = screen_delta / NODE_EDITOR_MIN_SCALE;
        let final_min = egui::pos2(origin_position[0] + graph_delta, origin_position[1]);
        let exact_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.4);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, exact_rect)]),
            &HashMap::from([(node_id, [final_min.x, final_min.y])]),
            exact_rect.center(),
            NODE_EDITOR_MIN_SCALE,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Clip(clip_id)
                && intent.target.kind == ReparentTargetKind::RetainedOrigin
        }));
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        assert!(reparent_nodes_from_intents(&mut project, &intents));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let estimated_rect = estimated_node_rect(&project, node_id);
        let content = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
        });
        assert!(content
            .zip(estimated_rect)
            .is_some_and(|(content, estimated)| rect_contains_rect(content, estimated)));
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn retained_growth_rolls_back_when_it_would_overlap_a_sibling_container() {
        let (mut project, composition_id, track_id, clip_id, _, node_id) = fixture();
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        apply_auto_layout(&mut project, composition_id, &plan);
        let first_clip = project.get_clip(clip_id).cloned();
        assert!(first_clip.is_some());
        let Some(first_clip) = first_clip else {
            return;
        };
        let first_rect = container_rect(first_clip.ui_position, first_clip.ui_size);
        let mut sibling = Clip::new("Sibling", 0.0, 5.0);
        sibling.ui_position = [
            first_rect.right() + AUTO_LAYOUT_CLIP_GAP,
            first_clip.ui_position[1],
        ];
        sibling.ui_size = first_clip.ui_size;
        let sibling_id = sibling.id;
        project.add_clip(sibling);
        assert!(project.attach_clip_to_track(track_id, sibling_id).is_ok());
        let sibling_rect = project
            .get_clip(sibling_id)
            .map(|clip| container_rect(clip.ui_position, clip.ui_size));
        assert!(sibling_rect.is_some());
        let Some(sibling_rect) = sibling_rect else {
            return;
        };
        ensure_container_hierarchy_contains(
            &mut project,
            NodeContainer::Track(track_id),
            sibling_rect,
        );
        assert!(!layout_needs_reflow(&project, composition_id));

        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let graph_delta = (NODE_REPARENT_DRAG_THRESHOLD - 1.0) / NODE_EDITOR_MIN_SCALE;
        let final_min = egui::pos2(origin_position[0] + graph_delta, origin_position[1]);
        let final_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.4);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, final_rect)]),
            &HashMap::from([(node_id, [final_min.x, final_min.y])]),
            final_rect.center(),
            NODE_EDITOR_MIN_SCALE,
        );
        assert_eq!(intents.len(), 1);
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let initial_clip_geometry = project
            .get_clip(clip_id)
            .map(|clip| (clip.ui_position, clip.ui_size));
        assert_eq!(
            finish_node_reparent(&mut project, &intents, Some(&gesture)),
            ReparentReleaseOutcome::Rejected
        );
        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some(origin_position)
        );
        assert_eq!(
            project
                .get_clip(clip_id)
                .map(|clip| (clip.ui_position, clip.ui_size)),
            initial_clip_geometry
        );
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn reparent_applies_when_only_node_overlap_requires_optional_auto_layout() {
        let (mut project, composition_id, track_id, clip_id, _, node_id) = fixture();
        let overlap_node = Node::new_merge("Intentional overlap");
        let overlap_node_id = overlap_node.id;
        project.add_node(overlap_node);
        assert!(project
            .attach_node_to_container(NodeContainer::Track(track_id), overlap_node_id)
            .is_ok());
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        assert!(apply_auto_layout(&mut project, composition_id, &plan));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));

        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        let final_position = project
            .get_node(overlap_node_id)
            .map(|node| node.ui_position);
        assert!(origin_position.is_some());
        assert!(final_position.is_some());
        let (Some(origin_position), Some(final_position)) = (origin_position, final_position)
        else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let final_rect = egui::Rect::from_min_size(
            egui::pos2(final_position[0], final_position[1]),
            estimated_node_size(&project, node_id),
        );
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, final_rect)]),
            &HashMap::from([(node_id, final_position)]),
            final_rect.center(),
            1.0,
        );
        assert!(intents
            .first()
            .is_some_and(|intent| { intent.target.container == NodeContainer::Track(track_id) }));
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = final_position;
        }

        assert_eq!(
            finish_node_reparent(&mut project, &intents, Some(&gesture)),
            ReparentReleaseOutcome::Applied
        );
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some(final_position)
        );
        assert!(layout_needs_reflow(&project, composition_id));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));
    }

    #[test]
    fn multi_node_drag_uses_primary_target_without_splitting_the_group() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [2_100.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_500.0, 1_400.0];
        }
        let Some(solid) = project.get_node(solid_id) else {
            return;
        };
        let Some(merge) = project.get_node(merge_id) else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([
                (
                    solid_id,
                    NodeEditorNodeDragOrigin {
                        container: NodeContainer::Clip(clip_id),
                        position: solid.ui_position,
                    },
                ),
                (
                    merge_id,
                    NodeEditorNodeDragOrigin {
                        container: NodeContainer::Clip(clip_id),
                        position: merge.ui_position,
                    },
                ),
            ]),
            primary_node_id: Some(solid_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let solid_min = egui::pos2(1_120.0, 470.0);
        let merge_min = egui::pos2(1_440.0, 470.0);
        let solid_rect =
            egui::Rect::from_min_size(solid_min, estimated_node_size(&project, solid_id));
        let merge_rect =
            egui::Rect::from_min_size(merge_min, estimated_node_size(&project, merge_id));
        let final_positions = HashMap::from([
            (solid_id, [solid_min.x, solid_min.y]),
            (merge_id, [merge_min.x, merge_min.y]),
        ]);
        let final_rects = HashMap::from([(solid_id, solid_rect), (merge_id, merge_rect)]);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &final_rects,
            &final_positions,
            solid_rect.center(),
            1.0,
        );
        assert_eq!(intents.len(), 2);
        assert!(intents
            .iter()
            .all(|intent| intent.target.container == NodeContainer::Track(track_id)));

        if let Some(node) = project.get_node_mut(solid_id) {
            node.ui_position = [solid_min.x, solid_min.y];
        }
        if let Some(node) = project.get_node_mut(merge_id) {
            node.ui_position = [merge_min.x, merge_min.y];
        }
        let connection_ids = project
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        assert!(reparent_nodes_from_intents(&mut project, &intents));
        assert_eq!(
            project.find_node_container(solid_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project.find_node_container(merge_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project
                .connections
                .iter()
                .map(|connection| connection.id)
                .collect::<Vec<_>>(),
            connection_ids
        );
        assert_eq!(
            project
                .get_node(merge_id)
                .map(|node| node.ui_position[0])
                .zip(project.get_node(solid_id).map(|node| node.ui_position[0]))
                .map(|(merge_x, solid_x)| merge_x - solid_x),
            Some(320.0)
        );
    }

    #[test]
    fn reparent_min_edge_growth_keeps_node_drop_position_and_contains_every_ancestor() {
        let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
        let other_node_ids = project
            .get_clip(clip_id)
            .map(|clip| {
                clip.node_ids
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != node_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for other_node_id in other_node_ids {
            project.remove_node(other_node_id);
        }
        let Some(composition) = project.get_composition_mut(composition_id) else {
            assert!(project.get_composition(composition_id).is_some());
            return;
        };
        composition.ui_position = [100.0, 100.0];
        composition.ui_size = [1_700.0, 1_300.0];
        let Some(track) = project.get_track_mut(track_id) else {
            assert!(project.get_track(track_id).is_some());
            return;
        };
        track.ui_position = [300.0, 240.0];
        track.ui_size = [1_100.0, 900.0];
        let Some(clip) = project.get_clip_mut(clip_id) else {
            assert!(project.get_clip(clip_id).is_some());
            return;
        };
        clip.ui_position = [490.0, 380.0];
        clip.ui_size = [800.0, 600.0];
        assert!(project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .is_ok());

        let clip_before = project.get_clip(clip_id).map(|clip| clip.ui_position);
        let track_before = project.get_track(track_id).map(|track| track.ui_position);
        let composition_before = project
            .get_composition(composition_id)
            .map(|composition| composition.ui_position);
        assert!(clip_before.is_some() && track_before.is_some() && composition_before.is_some());
        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_content = nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        );
        let node_size = estimated_node_size(&project, node_id);
        let exact_snarl_size = node_size * 0.45;
        let final_min = clip_content.min - exact_snarl_size * 0.4;
        let final_rect = egui::Rect::from_min_size(final_min, exact_snarl_size);
        let Some(node) = project.get_node_mut(node_id) else {
            return;
        };
        node.ui_position = [final_min.x, final_min.y];
        let target = deepest_legal_reparent_target(
            &project,
            composition_id,
            final_rect,
            clip_content.min + egui::vec2(2.0, 2.0),
        );
        assert!(
            target.is_some(),
            "partially overlapping final Node rect had no legal target"
        );
        let Some(target) = target else {
            return;
        };
        assert_eq!(target.container, NodeContainer::Clip(clip_id));
        assert!(reparent_nodes_from_intents(
            &mut project,
            &[NodeDropIntent {
                node_id,
                final_rect,
                target,
            }],
        ));

        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some([final_min.x, final_min.y])
        );
        assert!(project
            .get_clip(clip_id)
            .zip(clip_before)
            .is_some_and(|(clip, before)| {
                clip.ui_position[0] < before[0] && clip.ui_position[1] < before[1]
            }));
        assert!(project
            .get_track(track_id)
            .zip(track_before)
            .is_some_and(|(track, before)| {
                track.ui_position[0] < before[0] && track.ui_position[1] < before[1]
            }));
        assert!(project
            .get_composition(composition_id)
            .zip(composition_before)
            .is_some_and(|(composition, before)| {
                composition.ui_position[0] < before[0] && composition.ui_position[1] < before[1]
            }));

        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_rect = container_rect(clip.ui_position, clip.ui_size);
        let persisted_rect = egui::Rect::from_min_size(final_min, node_size);
        assert!(rect_contains_rect(
            nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP),
            final_rect,
        ));
        assert!(rect_contains_rect(
            nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP),
            persisted_rect,
        ));
        let Some(track) = project.get_track(track_id) else {
            return;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        assert!(rect_contains_rect(
            nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP),
            clip_rect,
        ));
        let Some(composition) = project.get_composition(composition_id) else {
            return;
        };
        assert!(rect_contains_rect(
            composition_content_rect(container_rect(composition.ui_position, composition.ui_size,)),
            track_rect,
        ));
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn composition_root_fallback_expands_same_owner_on_left_and_top() {
        let (mut project, composition_id, _, _, node_id, _) = fixture();
        assert!(project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .is_ok());
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        assert!(apply_auto_layout(&mut project, composition_id, &plan));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));
        let Some(origin_node) = project.get_node(node_id) else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Composition(composition_id),
                    position: origin_node.ui_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let final_min = egui::pos2(-800.0, -260.0);
        let estimated_size = estimated_node_size(&project, node_id);
        let final_rect = egui::Rect::from_min_size(final_min, estimated_size * 0.4);
        let final_positions = HashMap::from([(node_id, [final_min.x, final_min.y])]);
        let final_rects = HashMap::from([(node_id, final_rect)]);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &final_rects,
            &final_positions,
            final_rect.center(),
            1.0,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Composition(composition_id)
                && intent.target.root_fallback
                && intent.target.kind == ReparentTargetKind::RootFallback
        }));
        let reported = intents
            .first()
            .copied()
            .and_then(|active| reported_reparent_target(None, true, active));
        assert!(reported.is_some_and(|target| {
            target.kind == ReparentTargetKind::RootFallback
                && (target.score + 1.0).abs() <= f32::EPSILON
        }));
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let before = project
            .get_composition(composition_id)
            .map(|composition| composition.ui_position);
        assert!(reparent_nodes_from_intents(&mut project, &intents));
        assert!(project
            .get_composition(composition_id)
            .zip(before)
            .is_some_and(|(composition, before)| {
                composition.ui_position[0] < before[0]
                    && composition.ui_position[1] < before[1]
                    && rect_contains_rect(
                        composition_content_rect(container_rect(
                            composition.ui_position,
                            composition.ui_size,
                        )),
                        final_rect,
                    )
            }));
        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some([final_min.x, final_min.y])
        );
        let root_content = project.get_composition(composition_id).map(|composition| {
            composition_content_rect(container_rect(composition.ui_position, composition.ui_size))
        });
        assert!(root_content.is_some_and(|content| {
            rect_contains_rect(
                content,
                egui::Rect::from_min_size(final_min, estimated_size),
            )
        }));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));
    }

    #[test]
    fn reparent_clears_only_old_output_binding_and_preserves_typed_wire_identity_and_history() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        let mut track_output = Node::new_merge("Track Output");
        track_output.ui_position = [1_350.0, 420.0];
        let track_output_id = track_output.id;
        project.add_node(track_output);
        assert!(project
            .attach_node_to_container(NodeContainer::Track(track_id), track_output_id)
            .is_ok());
        assert!(project
            .set_output_node(NodeContainer::Track(track_id), Some(track_output_id))
            .is_ok());
        assert!(project
            .set_output_node(NodeContainer::Clip(clip_id), Some(solid_id))
            .is_ok());
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        let time_before = project
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT)
                    && connection.to.owner == PortOwner::Node(solid_id)
            })
            .cloned();
        let image_before = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .cloned();
        assert!(time_before.is_some() && image_before.is_some());
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());

        let final_min = egui::pos2(1_120.0, 470.0);
        let final_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, solid_id));
        if let Some(node) = project.get_node_mut(solid_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let target = deepest_legal_reparent_target(
            &project,
            composition_id,
            final_rect,
            final_rect.center(),
        );
        assert!(
            target.is_some(),
            "Track content did not resolve as a drop target"
        );
        let Some(target) = target else {
            return;
        };
        assert_eq!(target.container, NodeContainer::Track(track_id));
        assert!(reparent_nodes_from_intents(
            &mut project,
            &[NodeDropIntent {
                node_id: solid_id,
                final_rect,
                target,
            }],
        ));
        history.push_project_state(project.clone());

        assert_eq!(
            project.find_node_container(solid_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project
                .get_clip(clip_id)
                .and_then(|clip| clip.output_node_id),
            None
        );
        assert_eq!(
            project
                .get_track(track_id)
                .and_then(|track| track.output_node_id),
            Some(track_output_id)
        );
        assert_eq!(
            image_before.as_ref().and_then(|connection| {
                project
                    .connections
                    .iter()
                    .find(|candidate| candidate.id == connection.id)
            }),
            image_before.as_ref(),
        );
        let time_after = time_before.as_ref().and_then(|connection| {
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == connection.id)
        });
        assert!(time_after.is_some_and(|connection| {
            connection.id == time_before.as_ref().map_or(Uuid::nil(), |before| before.id)
                && connection.from == PortAddress::new(PortOwner::Track(track_id), TIME_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(solid_id), TIME_PORT)
        }));
        assert_eq!(project.connections.len(), initial.connections.len());
        assert!(project.validate_containment().is_empty());
        assert!(project.validate_connections().is_empty());
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn rejected_reparent_restores_all_live_positions_without_history_or_wire_mutation() {
        let (mut project, composition_id, _, clip_id, node_id, _) = fixture();
        let initial = project.clone();
        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let final_min = egui::pos2(origin_position[0] + 700.0, origin_position[1] + 120.0);
        let final_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.5);
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let target = deepest_legal_reparent_target(
            &project,
            composition_id,
            final_rect,
            final_rect.center(),
        );
        assert!(target.is_some());
        let Some(mut target) = target else {
            return;
        };
        target.container = NodeContainer::Clip(Uuid::from_u128(0xdead_beef));
        let outcome = finish_node_reparent(
            &mut project,
            &[NodeDropIntent {
                node_id,
                final_rect,
                target,
            }],
            Some(&gesture),
        );
        assert_eq!(outcome, ReparentReleaseOutcome::Rejected);
        let moved_node_ids = HashSet::from([node_id]);
        assert_eq!(
            splice_node_for_release(outcome, &moved_node_ids, Some(node_id)),
            None
        );
        assert_eq!(
            splice_node_for_release(ReparentReleaseOutcome::NoIntent, &moved_node_ids, None,),
            None
        );
        assert_eq!(
            splice_node_for_release(
                ReparentReleaseOutcome::NoIntent,
                &moved_node_ids,
                Some(node_id),
            ),
            Some(node_id)
        );
        assert_eq!(project, initial);
        let mut history = HistoryManager::new();
        history.push_project_state(initial);
        history.push_project_state(project);
        assert_eq!(history.undo_depth(), 1);
    }

    #[test]
    fn real_egui_capture_selects_the_top_overlapping_node_for_a_multi_drag() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        assert!(project.remove_node(solid_id).is_some());
        assert!(project.remove_node(merge_id).is_some());
        if let Some(clip) = project.get_clip_mut(clip_id) {
            clip.ui_size = [1_300.0, 760.0];
        }
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_050.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        let clip_content = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
        });
        assert!(clip_content.is_some());
        let Some(clip_content) = clip_content else {
            return;
        };
        let mut lower = Node::new_merge("Capture Lower");
        lower.id = Uuid::from_u128(1);
        lower.ui_position = [clip_content.min.x + 40.0, clip_content.min.y + 40.0];
        let lower_id = lower.id;
        let mut upper = Node::new_merge("Capture Upper");
        upper.id = Uuid::from_u128(2);
        upper.ui_position = [clip_content.min.x + 520.0, clip_content.min.y + 40.0];
        let upper_id = upper.id;
        project.add_node(lower);
        project.add_node(upper);
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), lower_id)
            .is_ok());
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), upper_id)
            .is_ok());

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let graph_id = egui::Id::new(("captured-overlap-drag", composition_id));
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
        reset_test_rects();
        let render_frame = |project: &Project,
                            snarl: &mut Snarl<GraphItem>,
                            frame: usize,
                            events: Vec<egui::Event>,
                            modifiers: egui::Modifiers| {
            let mut layout_edits = Vec::new();
            let mut captured = None;
            let mut persistent_snarl_id = None;
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            rendered_node_rects: Arc::clone(&rendered_node_rects),
                        };
                        snarl.show(&mut viewer, &node_editor_snarl_style(), graph_id, ui);
                        drop(viewer);
                        let snarl_id = ui.make_persistent_id(graph_id);
                        persistent_snarl_id = Some(snarl_id);
                        captured = captured_snarl_drag_node(ui.ctx(), snarl, snarl_id);
                        layout_edits = collect_layout_edits(project, snarl);
                    });
                },
            ));
            (layout_edits, captured, persistent_snarl_id)
        };

        let mut persistent_snarl_id = None;
        for frame in 0..4 {
            let (_, _, frame_snarl_id) = render_frame(
                &project,
                &mut snarl,
                frame,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            persistent_snarl_id = frame_snarl_id;
        }
        let lower_rect = test_rect(&format!("node_editor.node:{lower_id}"));
        let upper_rect = test_rect(&format!("node_editor.node:{upper_id}"));
        assert!(lower_rect.is_some_and(|rect| rect.is_positive()));
        assert!(upper_rect.is_some_and(|rect| rect.is_positive()));
        assert!(test_rect(&format!("node_editor.node_header:{upper_id}"))
            .is_some_and(|rect| rect.is_positive()));
        let Some(persistent_snarl_id) = persistent_snarl_id else {
            return;
        };
        let snarl_node_ids = snarl
            .node_ids()
            .filter_map(|(snarl_id, item)| match item {
                GraphItem::Node(project_id) if [lower_id, upper_id].contains(project_id) => {
                    Some((*project_id, snarl_id))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let (Some(lower_snarl_id), Some(upper_snarl_id)) = (
            snarl_node_ids.get(&lower_id).copied(),
            snarl_node_ids.get(&upper_id).copied(),
        ) else {
            return;
        };
        // Establish the group through Snarl's real rectangle-selection path.
        // Starting in the canvas margin avoids invisible container controls;
        // selecting the other graph items too is harmless for this capture
        // test and exercises the same group-drag path.
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut frame = 4;
        let selection_start = screen.min + egui::vec2(20.0, 20.0);
        let selection_drag_start = selection_start + egui::vec2(10.0, 10.0);
        let selection_end = screen.max - egui::vec2(20.0, 20.0);
        for events in [
            vec![egui::Event::PointerMoved(selection_start)],
            vec![egui::Event::PointerButton {
                pos: selection_start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: shift,
            }],
            vec![egui::Event::PointerMoved(selection_drag_start)],
            vec![egui::Event::PointerMoved(selection_end)],
            vec![egui::Event::PointerButton {
                pos: selection_end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: shift,
            }],
        ] {
            render_frame(&project, &mut snarl, frame, events, shift);
            frame += 1;
        }
        let selected = egui_snarl::ui::get_selected_nodes(persistent_snarl_id, &context);
        assert!(
            selected.contains(&lower_snarl_id),
            "lower {lower_snarl_id:?} selection missing from {selected:?}; upper is {upper_snarl_id:?}"
        );
        assert!(
            selected.contains(&upper_snarl_id),
            "upper selection missing from {selected:?}"
        );
        // A newly visible body changes Snarl's measured Node width while its
        // open animation settles. Coordinate input must use geometry from a
        // settled frame, just like the HTTP QA client does.
        let mut previous_upper_header = None;
        let mut stable_header_frames = 0;
        let mut settled_upper_header = None;
        for _ in 0..30 {
            render_frame(
                &project,
                &mut snarl,
                frame,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            frame += 1;
            let current = test_rect(&format!("node_editor.node_header:{upper_id}"));
            if current.is_some_and(|rect| rect.is_positive()) && current == previous_upper_header {
                stable_header_frames += 1;
                if stable_header_frames >= 2 {
                    settled_upper_header = current;
                    break;
                }
            } else {
                stable_header_frames = 0;
            }
            previous_upper_header = current;
        }
        let Some(upper_header) = settled_upper_header else {
            panic!("Merge header geometry did not settle before coordinate input");
        };
        assert!(upper_header.is_positive());
        let upper_header_center = upper_header.center();
        let command = egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        for events in [
            vec![egui::Event::PointerMoved(upper_header_center)],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: command,
            }],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: command,
            }],
        ] {
            render_frame(&project, &mut snarl, frame, events, command);
            frame += 1;
        }
        let selected_after_command =
            egui_snarl::ui::get_selected_nodes(persistent_snarl_id, &context);
        assert!(selected_after_command.contains(&lower_snarl_id));
        assert!(!selected_after_command.contains(&upper_snarl_id));
        let post_show_project_ids = selected_after_command
            .iter()
            .filter_map(|snarl_id| match snarl.get_node(*snarl_id) {
                Some(GraphItem::Node(node_id)) => Some(*node_id),
                Some(GraphItem::Container(_) | GraphItem::PortAnchor { .. }) | None => None,
            })
            .collect::<Vec<_>>();
        let (typed_targets, typed_primary) = node_selection_after_snarl_click(
            &[
                SelectionTarget::Node(lower_id),
                SelectionTarget::Node(upper_id),
            ],
            Some(SelectionTarget::Node(upper_id)),
            &post_show_project_ids,
            upper_id,
            command,
        );
        assert_eq!(typed_targets, vec![SelectionTarget::Node(lower_id)]);
        assert_eq!(typed_primary, Some(SelectionTarget::Node(lower_id)));

        // Restore the group for the overlapping multi-drag assertion below.
        let upper_header = test_rect(&format!("node_editor.node_header:{upper_id}"));
        assert!(upper_header.is_some_and(|rect| rect.is_positive()));
        let Some(upper_header) = upper_header else {
            return;
        };
        let upper_header_center = upper_header.center();
        for events in [
            vec![egui::Event::PointerMoved(upper_header_center)],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: shift,
            }],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: shift,
            }],
        ] {
            render_frame(&project, &mut snarl, frame, events, shift);
            frame += 1;
        }

        let overlap_position = egui::pos2(clip_content.min.x + 260.0, clip_content.min.y + 220.0);
        for node_id in [lower_id, upper_id] {
            if let Some(node) = project.get_node_mut(node_id) {
                node.ui_position = [overlap_position.x, overlap_position.y];
            }
        }
        for node_id in [lower_id, upper_id] {
            if let Some(snarl_id) = snarl_node_ids.get(&node_id).copied() {
                if let Some(node) = snarl.get_node_info_mut(snarl_id) {
                    node.pos = overlap_position;
                }
            }
        }
        for _ in 0..2 {
            render_frame(
                &project,
                &mut snarl,
                frame,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            frame += 1;
        }
        let top_header = test_rect(&format!("node_editor.node_header:{upper_id}"));
        assert!(top_header.is_some_and(|rect| rect.is_positive()));
        let Some(top_header) = top_header else {
            return;
        };
        let start = top_header.center();
        render_frame(
            &project,
            &mut snarl,
            frame,
            vec![egui::Event::PointerMoved(start)],
            egui::Modifiers::NONE,
        );
        frame += 1;
        render_frame(
            &project,
            &mut snarl,
            frame,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
        frame += 1;
        let end = start + egui::vec2(48.0, 24.0);
        let (layout_edits, captured, _) = render_frame(
            &project,
            &mut snarl,
            frame,
            vec![egui::Event::PointerMoved(end)],
            egui::Modifiers::NONE,
        );
        assert_eq!(captured, Some(upper_id));
        assert_ne!(captured, Some(lower_id));
        let moved_nodes = layout_edits
            .iter()
            .filter_map(|edit| match edit {
                LayoutEdit::MoveNode { node_id, .. }
                    if *node_id == lower_id || *node_id == upper_id =>
                {
                    Some(*node_id)
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        assert_eq!(moved_nodes, HashSet::from([lower_id, upper_id]));
        let mut state = NodeEditorState::default();
        record_node_reparent_origins(&project, &layout_edits, &mut state, true);
        if let Some(gesture) = state.node_reparent.as_mut() {
            gesture.primary_node_id =
                captured.filter(|node_id| gesture.origins.contains_key(node_id));
        }
        assert_eq!(
            state
                .node_reparent
                .as_ref()
                .and_then(|gesture| gesture.primary_node_id),
            Some(upper_id)
        );
    }

    #[test]
    fn cmd_deselect_uses_post_snarl_state_without_reselecting_clicked_node() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let current = [SelectionTarget::Node(first), SelectionTarget::Node(second)];
        let command = egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        };

        let (targets, primary) = node_selection_after_snarl_click(
            &current,
            Some(SelectionTarget::Node(second)),
            &[first],
            second,
            command,
        );

        assert_eq!(targets, vec![SelectionTarget::Node(first)]);
        assert_eq!(primary, Some(SelectionTarget::Node(first)));
    }

    #[test]
    fn shift_node_selection_keeps_same_uuid_non_node_target() {
        let shared_id = Uuid::from_u128(1);
        let other_node_id = Uuid::from_u128(2);
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };

        let (targets, primary) = node_selection_after_snarl_click(
            &[SelectionTarget::Clip(shared_id)],
            Some(SelectionTarget::Clip(shared_id)),
            &[other_node_id, shared_id, shared_id],
            shared_id,
            shift,
        );

        assert_eq!(
            targets,
            vec![
                SelectionTarget::Clip(shared_id),
                SelectionTarget::Node(other_node_id),
                SelectionTarget::Node(shared_id),
            ]
        );
        assert_eq!(primary, Some(SelectionTarget::Node(shared_id)));
    }

    #[test]
    fn real_egui_node_header_drag_reparents_once_from_final_snarl_rect() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        assert!(project
            .set_output_node(NodeContainer::Clip(clip_id), Some(solid_id))
            .is_ok());
        let explicit_wire = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .cloned();
        assert!(explicit_wire.is_some());
        let initial = project.clone();
        let initial_position = project.get_node(solid_id).map(|node| node.ui_position);
        assert!(initial_position.is_some());
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut final_transform = egui::emath::TSTransform::IDENTITY;
        reset_test_rects();

        for frame in 0..5 {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            rendered_node_rects: Arc::clone(&rendered_node_rects),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("real-reparent-drag", composition_id)),
                            ui,
                        );
                        final_transform = to_global;
                    });
                },
            ));
        }

        let header = test_rect(&format!("node_editor.node_header:{solid_id}"));
        assert!(header.is_some_and(|rect| rect.is_positive()));
        let Some(header) = header else {
            return;
        };
        let Some(initial_position) = initial_position else {
            return;
        };
        let start = header.center();
        let desired_position = [1_120.0, 470.0];
        let graph_delta = egui::vec2(
            desired_position[0] - initial_position[0],
            desired_position[1] - initial_position[1],
        );
        let end = start + graph_delta * final_transform.scaling;
        assert!(screen.contains(end));
        let drag_start = start + (end - start).normalized() * 12.0;
        let event_frames = [
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(drag_start)],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let mut history_commits = 0;
        let mut release_outcome = ReparentReleaseOutcome::NoIntent;
        for (offset, events) in event_frames.into_iter().enumerate() {
            let mut frame_layout_edits = Vec::new();
            let mut frame_drop_intents = Vec::new();
            let mut frame_released = false;
            if let Ok(mut rects) = rendered_node_rects.lock() {
                rects.clear();
            }
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((offset + 5) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            rendered_node_rects: Arc::clone(&rendered_node_rects),
                        };
                        let graph_id = egui::Id::new(("real-reparent-drag", composition_id));
                        snarl.show(&mut viewer, &node_editor_snarl_style(), graph_id, ui);
                        drop(viewer);
                        let captured_drag_node_id = captured_snarl_drag_node(
                            ui.ctx(),
                            &snarl,
                            ui.make_persistent_id(graph_id),
                        );
                        frame_layout_edits = collect_layout_edits(&project, &snarl);
                        let (primary_down, primary_released, pointer) = ui.input(|input| {
                            (
                                input.pointer.primary_down(),
                                input.pointer.primary_released(),
                                input.pointer.interact_pos(),
                            )
                        });
                        frame_released = primary_released;
                        record_node_reparent_origins(
                            &project,
                            &frame_layout_edits,
                            &mut state,
                            primary_down || primary_released,
                        );
                        let Some(pointer) = pointer else {
                            return;
                        };
                        let graph_point = to_global.inverse() * pointer;
                        let Ok(rects) = rendered_node_rects.lock() else {
                            return;
                        };
                        if let Some(gesture) = state.node_reparent.as_mut() {
                            if gesture.primary_node_id.is_none() {
                                gesture.primary_node_id = captured_drag_node_id
                                    .filter(|node_id| gesture.origins.contains_key(node_id));
                            }
                        }
                        if let Some(gesture) = state.node_reparent.as_ref() {
                            frame_drop_intents = node_drop_intents(
                                &project,
                                composition_id,
                                gesture,
                                &rects,
                                &final_node_positions(&project, gesture, &frame_layout_edits),
                                graph_point,
                                to_global.scaling,
                            );
                        }
                    });
                },
            ));
            let mut frame_changed = false;
            for edit in frame_layout_edits {
                frame_changed |= apply_layout_edit(&mut project, edit);
            }
            if frame_released {
                let reparent_gesture = state.node_reparent.take();
                release_outcome = finish_node_reparent(
                    &mut project,
                    &frame_drop_intents,
                    reparent_gesture.as_ref(),
                );
                frame_changed |= release_outcome != ReparentReleaseOutcome::NoIntent;
                state.moved_node_ids.clear();
                if frame_changed {
                    history.push_project_state(project.clone());
                    history_commits += 1;
                }
            }
        }

        assert_eq!(release_outcome, ReparentReleaseOutcome::Applied);
        assert_eq!(history_commits, 1);
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(
            project.find_node_container(solid_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project
                .get_clip(clip_id)
                .and_then(|clip| clip.output_node_id),
            None
        );
        assert_eq!(
            explicit_wire.as_ref().and_then(|wire| {
                project
                    .connections
                    .iter()
                    .find(|connection| connection.id == wire.id)
            }),
            explicit_wire.as_ref(),
        );
        assert!(project
            .get_node(solid_id)
            .is_some_and(|node| node.ui_position != initial_position));
        assert!(project.validate_containment().is_empty());
        assert!(project.validate_connections().is_empty());
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn root_scoped_creation_expands_a_collapsed_composition() {
        let (mut project, composition_id, track_id, clip_id, _, _) = fixture();
        project.get_track_mut(track_id).unwrap().ui_collapsed = true;
        let hidden_track_body = egui::pos2(500.0, 400.0);
        project
            .get_composition_mut(composition_id)
            .unwrap()
            .ui_collapsed = true;
        let node = Node::new_merge("Root Node");
        let node_id = node.id;
        create_prebuilt_node(&mut project, hidden_track_body, node, composition_id);

        let composition = project.get_composition(composition_id).unwrap();
        assert!(!composition.ui_collapsed);
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Composition(composition_id))
        );
        assert!(project.validate_containment().is_empty());
        let (snarl, _) = build_snarl(&project, composition_id);
        assert!(snarl.nodes().any(|item| *item == GraphItem::Node(node_id)));

        // The original Clip remains owned by the collapsed Track; creation did
        // not mutate hierarchy outside the new Node's single owner.
        assert_eq!(project.find_track_for_clip(clip_id), Some(track_id));
    }

    #[test]
    fn locally_created_node_preserves_every_existing_position_and_stays_visible() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        project.get_clip_mut(clip_id).unwrap().ui_collapsed = true;

        let composition_before = project.get_composition(composition_id).unwrap().clone();
        let track_before = project.get_track(track_id).unwrap().clone();
        let clip_before = project.get_clip(clip_id).unwrap().clone();
        let solid_before = project.get_node(solid_id).unwrap().clone();
        let merge_before = project.get_node(merge_id).unwrap().clone();
        let connections_before = project.connections.clone();
        let desired = egui::pos2(500.0, 400.0);
        let node = generator_node(
            "Locally Placed Text",
            GeneratorNodeRequest::Text {
                text: "Locally Placed Text".to_string(),
                font: "Arial".to_string(),
            },
        );
        let node_id = node.id;

        create_prebuilt_node(&mut project, desired, node, composition_id);

        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project.get_composition(composition_id).unwrap().ui_position,
            composition_before.ui_position
        );
        assert_eq!(
            project.get_track(track_id).unwrap().ui_position,
            track_before.ui_position
        );
        assert_eq!(
            project.get_clip(clip_id).unwrap().ui_position,
            clip_before.ui_position
        );
        assert_eq!(
            project.get_node(solid_id).unwrap().ui_position,
            solid_before.ui_position
        );
        assert_eq!(
            project.get_node(merge_id).unwrap().ui_position,
            merge_before.ui_position
        );
        assert_eq!(project.connections, connections_before);

        let (snarl, _) = build_snarl(&project, composition_id);
        assert!(snarl.nodes().any(|item| *item == GraphItem::Node(node_id)));
    }

    #[test]
    fn rendered_ports_stay_vertical_on_their_left_and_right_sides_with_bounded_width() {
        let (project, composition_id, _, _, node_id, _) = fixture();
        let context = egui::Context::default();
        reset_test_rects();

        for frame in 0..3 {
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 1200.0),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            drop(context.run(raw_input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let (mut snarl, containers) = build_snarl(&project, composition_id);
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
                    let mut wire_context_request = None;
                    let mut context_menu_exclusion_rects = Vec::new();
                    let mut to_global = egui::emath::TSTransform::default();
                    let mut canvas_clip = ui.clip_rect();
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        pending_selection: &mut selection,
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                    };
                    let style = SnarlStyle {
                        collapsible: Some(false),
                        max_scale: Some(1.0),
                        ..Default::default()
                    };
                    snarl.show(
                        &mut viewer,
                        &style,
                        egui::Id::new(("node-layout-test", composition_id)),
                        ui,
                    );
                });
            }));
        }

        let node = test_rect(&format!("node_editor.node:{node_id}")).expect("rendered node rect");
        let time_input = test_rect(&format!(
            "node_editor.port.node:{node_id}.input:{TIME_PORT}"
        ))
        .expect("time input pin rect");
        let image_output = test_rect(&format!(
            "node_editor.port.node:{node_id}.output:{IMAGE_OUTPUT_PORT}"
        ))
        .expect("image output pin rect");

        assert!(time_input.center().x < node.center().x);
        assert!(image_output.center().x > node.center().x);
        assert!(node.width() <= 500.0, "{node:?}");
    }

    #[test]
    fn qa_geometry_is_clipped_to_the_actual_node_editor_canvas() {
        let canvas = egui::Rect::from_min_max(egui::pos2(100.0, 200.0), egui::pos2(500.0, 400.0));
        let partial = egui::Rect::from_min_max(egui::pos2(80.0, 250.0), egui::pos2(180.0, 450.0));
        assert_eq!(
            clipped_qa_rect(partial, canvas),
            egui::Rect::from_min_max(egui::pos2(100.0, 250.0), egui::pos2(180.0, 400.0))
        );

        let outside = egui::Rect::from_min_max(egui::pos2(700.0, 20.0), egui::pos2(760.0, 80.0));
        let clipped = clipped_qa_rect(outside, canvas);
        assert!(!clipped.is_positive());
        assert_eq!(clipped.min, egui::pos2(canvas.right(), canvas.top()));
        assert_eq!(clipped.max, clipped.min);
        assert!(canvas.contains(clipped.center()));
    }
}
