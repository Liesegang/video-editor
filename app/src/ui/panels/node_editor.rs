use crate::action::HistoryManager;
use crate::state::context_types::{
    ContainerResizeEdge, ContainerResizeState, ContextMenuState, NodeEditorPendingEdit,
    NodeEditorState, SelectionState,
};
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};
use eframe::egui::{self, Color32};
use egui_snarl::{
    ui::{
        BackgroundPattern, NodeLayout, PinInfo, PinPlacement, PinWireInfo, SnarlPin, SnarlStyle,
        SnarlViewer, WireLayer, WireStyle,
    },
    InPin, OutPin, Snarl,
};
use library::model::project::{
    ContainerImageSourceKind, PortAddress, PortDataType, PortDirection, PortOwner, PortSide,
};
use library::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use library::model::{
    Clip, GeneratorContent, Node, NodeContainer, NodeContent, NodeGraphBundle, Project,
};
use library::plugin::{
    property_name_from_port, PluginManager, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY,
};
use library::EditorService;
use ordered_float::OrderedFloat;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

const CONTAINER_HEADER_HEIGHT: f32 = 64.0;
/// The previous 0.65 lower bound made an overview of a large graph impossible.
/// 0.0065 is exactly two orders of magnitude farther out while remaining far
/// enough above zero for stable inverse transforms.
const NODE_EDITOR_MIN_SCALE: f32 = 0.0065;
const NODE_EDITOR_MAX_SCALE: f32 = 1.25;
const NODE_EDITOR_MAX_TRANSLATION: f32 = 10_000_000.0;
const GRID_TARGET_SCREEN_SPACING: f32 = 52.0;
const NODE_EDITOR_DETAIL_SCALE: f32 = 0.18;
const NODE_EDITOR_RESIZE_INTERACTION_SCALE: f32 = 0.12;
const CONTAINER_CONTROL_OFFSET: egui::Vec2 = egui::vec2(14.0, 10.0);
const CONTAINER_PORT_Y: f32 = 86.0;
const EMBEDDED_PORT_LABEL_INSET: f32 = 18.0;
const RESIZE_HIT_WIDTH: f32 = 7.0;
const RESIZE_CORNER_SIZE: f32 = 15.0;
const NODE_BODY_WIDTH: f32 = 200.0;
const NODE_HEADER_WIDTH: f32 = 190.0;
const PORT_LABEL_WIDTH: f32 = 96.0;
const PORT_ROW_HEIGHT: f32 = 22.0;
const PROPERTY_LABEL_WIDTH: f32 = 58.0;
const INLINE_CONTROL_WIDTH: f32 = 126.0;
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

fn sanitized_node_editor_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale.clamp(NODE_EDITOR_MIN_SCALE, NODE_EDITOR_MAX_SCALE)
    } else {
        1.0
    }
}

fn sanitize_node_editor_transform(transform: &mut egui::emath::TSTransform) {
    transform.scaling = sanitized_node_editor_scale(transform.scaling);
    for value in [&mut transform.translation.x, &mut transform.translation.y] {
        *value = if value.is_finite() {
            value.clamp(-NODE_EDITOR_MAX_TRANSLATION, NODE_EDITOR_MAX_TRANSLATION)
        } else {
            0.0
        };
    }
}

/// Pick a 1/2/5-decade grid size in graph units. This keeps the number of
/// painted lines proportional to screen size instead of exploding at 0.0065x.
fn adaptive_grid_spacing(scale: f32) -> f32 {
    let target = GRID_TARGET_SCREEN_SPACING / sanitized_node_editor_scale(scale);
    let decade = 10.0_f32.powf(target.log10().floor());
    let normalized = target / decade;
    let multiplier = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    (decade * multiplier).clamp(1.0, 1_000_000_000.0)
}

fn screen_stroke_in_graph_units(screen_width: f32, scale: f32) -> f32 {
    screen_width / sanitized_node_editor_scale(scale)
}

fn node_editor_details_visible(scale: f32) -> bool {
    sanitized_node_editor_scale(scale) >= NODE_EDITOR_DETAIL_SCALE
}

fn node_editor_port_interactions_enabled(scale: f32) -> bool {
    node_editor_details_visible(scale)
}

fn node_editor_resize_interactions_enabled(scale: f32) -> bool {
    sanitized_node_editor_scale(scale) >= NODE_EDITOR_RESIZE_INTERACTION_SCALE
}

fn node_editor_snarl_style() -> SnarlStyle {
    SnarlStyle {
        collapsible: Some(false),
        pin_placement: Some(PinPlacement::Edge),
        pin_size: Some(13.0),
        wire_width: Some(3.0),
        wire_style: Some(WireStyle::Bezier3),
        wire_layer: Some(WireLayer::BehindNodes),
        wire_frame_size: Some(72.0),
        bg_pattern: Some(BackgroundPattern::NoPattern),
        min_scale: Some(NODE_EDITOR_MIN_SCALE),
        max_scale: Some(NODE_EDITOR_MAX_SCALE),
        ..Default::default()
    }
}

fn node_editor_canvas_metadata(
    composition_id: Uuid,
    mut transform: egui::emath::TSTransform,
) -> serde_json::Value {
    sanitize_node_editor_transform(&mut transform);
    let scale = transform.scaling;
    serde_json::json!({
        "composition_id": composition_id,
        "scale": scale,
        "translation": {
            "x": transform.translation.x,
            "y": transform.translation.y,
        },
        "min_scale": NODE_EDITOR_MIN_SCALE,
        "max_scale": NODE_EDITOR_MAX_SCALE,
        "detail_enabled": node_editor_details_visible(scale),
        "port_interaction_enabled": node_editor_port_interactions_enabled(scale),
        "resize_interaction_enabled": node_editor_resize_interactions_enabled(scale),
    })
}

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
        let global_rect = clipped_qa_rect(unclipped_global_rect, self.canvas_clip);
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
        capture_test_rect(&self.component_id, global_rect);
        crate::qa::register_component_with_metadata(
            self.component_id.clone(),
            "node_port",
            global_rect,
            true,
            Some(serde_json::json!({
                "connected": self.connected,
                "direction": match self.direction {
                    PortDirection::Input => "input",
                    PortDirection::Output => "output",
                },
                "unclipped_rect": qa_rect_metadata(unclipped_global_rect),
                "visible_in_canvas": global_rect.is_positive(),
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
}

#[cfg(test)]
fn capture_test_rect(id: &str, rect: egui::Rect) {
    TEST_RENDER_RECTS.with(|rects| {
        rects.borrow_mut().insert(id.to_string(), rect);
    });
}

#[cfg(test)]
fn reset_test_rects() {
    TEST_RENDER_RECTS.with(|rects| rects.borrow_mut().clear());
}

#[cfg(test)]
fn test_rect(id: &str) -> Option<egui::Rect> {
    TEST_RENDER_RECTS.with(|rects| rects.borrow().get(id).copied())
}

#[cfg(test)]
fn test_rects() -> HashMap<String, egui::Rect> {
    TEST_RENDER_RECTS.with(|rects| rects.borrow().clone())
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
    SetOutputNode {
        owner: PortOwner,
        node_id: Option<Uuid>,
    },
    Delete {
        owner: PortOwner,
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
    to_global: &'a mut egui::emath::TSTransform,
    canvas_clip: &'a mut egui::Rect,
    rendered_ports: Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
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
                        if let NodeContent::Reference(reference) = &node.content {
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
        let _ = item;
        false
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
                let edit = response.changed().then(|| NodeEdit::Rename {
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

            match &node.content {
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
                    let color = node
                        .properties
                        .get("color")
                        .and_then(|property| {
                            property
                                .evaluate_at(property_time)
                                .get_as::<library::model::frame::color::Color>()
                        })
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
        if let Some(edit) = edit_for_wire(
            self.project,
            snarl,
            from.id.node,
            from.id.output,
            to.id.node,
            to.id.input,
            false,
        ) {
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
        sanitize_node_editor_transform(to_global);
        *self.to_global = *to_global;
    }
}

impl ProjectNodeViewer<'_> {
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
        let value = self
            .project
            .get_node(node_id)
            .and_then(|node| node.properties.get(property_key))
            .map(|property| property.evaluate_at(property_time));
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
            let NodeContent::PluginOperation(operation) = &node.content else {
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
                .then(|| NodeEdit::RenameContainer { owner, name });
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
                    let config = node_timing_drag_config(definition);
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
        let mut value = node
            .properties
            .get(key)
            .and_then(|property| property.evaluate_at(property_time).get_as::<String>())
            .unwrap_or_else(|| fallback.to_string());
        ui.horizontal(|ui| {
            property_label(ui, label);
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

fn node_palette(project: &Project, node_id: Uuid) -> NodePalette {
    match project.get_node(node_id).map(|node| &node.content) {
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
        Some(NodeContent::Merge) | None => NodePalette {
            body: Color32::from_rgb(38, 39, 43),
            header: Color32::from_rgb(68, 70, 79),
            accent: Color32::from_rgb(177, 182, 198),
        },
    }
}

fn node_icon(project: &Project, node_id: Uuid) -> &'static str {
    match project.get_node(node_id).map(|node| &node.content) {
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
    let NodeContent::PluginOperation(operation) = &node.content else {
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
) -> FloatDragValueConfig {
    FloatDragValueConfig::from_definition(definition)
        .expect("Clip timing definition has Float drag metadata")
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
) {
    let Ok(ports) = rendered_ports.lock() else {
        return;
    };
    for connection in &project.connections {
        register_edge_component(
            format!("node_editor.edge:{}", connection.id),
            "explicit",
            &connection.from,
            &connection.to,
            &ports,
            Some(connection.id),
            canvas_clip,
            project
                .port_definition(&connection.from, PortDirection::Output)
                .map_or_else(
                    || pin_color(PortDataType::Any),
                    |definition| pin_color(definition.data_type),
                ),
            overview,
        );
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
                ContainerImageSourceKind::OutputBinding => (
                    format!(
                        "node_editor.edge.output_binding:{}:{}",
                        qa_container_key(owner),
                        match source.source {
                            PortOwner::Node(node_id) => node_id.to_string(),
                            _ => source_key.clone(),
                        },
                    ),
                    "output_binding",
                ),
                ContainerImageSourceKind::DerivedChild => (
                    format!(
                        "node_editor.edge.derived:{}:{source_key}",
                        qa_container_key(owner)
                    ),
                    "derived_output",
                ),
            };
            register_edge_component(
                id,
                kind,
                &from,
                &sink,
                &ports,
                None,
                canvas_clip,
                pin_color(PortDataType::Image),
                overview,
            );
        }
    }
}

fn register_edge_component(
    id: String,
    kind: &str,
    from: &PortAddress,
    to: &PortAddress,
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    connection_id: Option<Uuid>,
    canvas_clip: egui::Rect,
    wire_color: Color32,
    overview: Option<OverviewWirePainter<'_>>,
) {
    let Some(from_rect) = ports.get(&RenderedPortKey {
        address: from.clone(),
        direction: PortDirection::Output,
    }) else {
        return;
    };
    let Some(to_rect) = ports.get(&RenderedPortKey {
        address: to.clone(),
        direction: PortDirection::Input,
    }) else {
        return;
    };
    let start = from_rect.center();
    let end = to_rect.center();
    if ![start, end]
        .iter()
        .all(|position| position.x.is_finite() && position.y.is_finite())
    {
        return;
    }
    let min_frame = if overview.is_some() { 2.0 } else { 36.0 };
    let frame = ((end.x - start.x).abs() * 0.45).clamp(min_frame, 110.0);
    let control_a = start + egui::vec2(frame, 0.0);
    let control_b = end - egui::vec2(frame, 0.0);
    let screen_points = [start, control_a, control_b, end];
    if let Some(overview) = overview {
        if let Some(graph_points) = overview_wire_graph_points(screen_points, overview.to_global) {
            let width = if kind == "derived_output" { 1.15 } else { 1.65 };
            overview
                .painter
                .add(egui::epaint::CubicBezierShape::from_points_stroke(
                    graph_points,
                    false,
                    Color32::TRANSPARENT,
                    egui::Stroke::new(
                        screen_stroke_in_graph_units(width, overview.to_global.scaling),
                        wire_color.gamma_multiply(0.9),
                    ),
                ));
        }
    }
    let unclipped_bbox = egui::Rect::from_points(&[start, control_a, control_b, end]).expand(7.0);
    let bbox = clipped_qa_rect(unclipped_bbox, canvas_clip);
    #[cfg(test)]
    capture_test_rect(&id, bbox);
    crate::qa::register_component_with_metadata(
        id,
        "node_edge",
        bbox,
        true,
        Some(serde_json::json!({
            "kind": kind,
            "connection_id": connection_id,
            "from": {
                "owner": qa_container_key(from.owner),
                "port": from.port,
                "x": start.x,
                "y": start.y,
            },
            "to": {
                "owner": qa_container_key(to.owner),
                "port": to.port,
                "x": end.x,
                "y": end.y,
            },
            "ltr": start.x <= end.x,
            "visible": bbox.is_positive(),
            "overview_painted": overview.is_some(),
            "unclipped_rect": qa_rect_metadata(unclipped_bbox),
        })),
    );
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

pub fn node_editor_panel(
    ui: &mut egui::Ui,
    comp_id: Option<Uuid>,
    project_lock: &Arc<RwLock<Project>>,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    selection: &mut SelectionState,
    current_time: f64,
    context_menu_state: &mut Option<ContextMenuState>,
    node_editor_state: &mut NodeEditorState,
) {
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
                project.find_containing_composition(pending.owner.id()) != Some(comp_id)
            })
        });
    if pending_owner_left_composition {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    let mut requested_layout = None;
    let mut selected_nodes = selection
        .selected_entities
        .iter()
        .copied()
        .collect::<Vec<_>>();
    selected_nodes.sort_unstable();
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
            requested_layout = Some(AutoLayoutScope::Container(
                selection
                    .last_selected_track_id
                    .map(PortOwner::Track)
                    .unwrap_or(PortOwner::Composition(comp_id)),
            ));
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
    let mut edits = Vec::new();
    let mut pending_selection = None;
    let mut context_menu_exclusion_rects = Vec::new();
    let mut to_global = egui::emath::TSTransform::default();
    let mut canvas_clip = canvas_rect;
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
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
            to_global: &mut to_global,
            canvas_clip: &mut canvas_clip,
            rendered_ports: Arc::clone(&rendered_ports),
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
        register_rendered_edges(&project, &rendered_ports, canvas_clip, overview);
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

        let mut collected = collect_layout_edits(&project, &snarl);
        collected.extend(container_resize_interactions(
            ui,
            &project,
            &containers,
            to_global,
            canvas_clip,
            node_editor_state,
        ));
        layout_edits = collected;
    }

    let selection_changed = pending_selection.is_some();
    if let Some(owner) = pending_selection {
        if let Ok(project) = project_lock.read() {
            match owner {
                PortOwner::Node(node_id) if project.get_node(node_id).is_some() => {
                    selection.selected_entities.clear();
                    selection.selected_entities.insert(node_id);
                    selection.last_selected_entity_id = Some(node_id);
                    selection.last_selected_track_id = project.find_parent_track(node_id);
                }
                PortOwner::Clip(clip_id) if project.get_clip(clip_id).is_some() => {
                    selection.selected_entities.clear();
                    selection.selected_entities.insert(clip_id);
                    selection.last_selected_entity_id = Some(clip_id);
                    selection.last_selected_track_id = project.find_track_for_clip(clip_id);
                }
                PortOwner::Track(track_id) if project.get_track(track_id).is_some() => {
                    selection.selected_entities.clear();
                    selection.last_selected_entity_id = None;
                    selection.last_selected_track_id = Some(track_id);
                }
                PortOwner::Composition(composition_id)
                    if project.get_composition(composition_id).is_some() =>
                {
                    selection.composition_id = Some(composition_id);
                }
                _ => {}
            }
        }
    }

    for edit in &layout_edits {
        if let LayoutEdit::MoveNode { node_id, .. } = edit {
            node_editor_state.moved_node_ids.insert(*node_id);
        }
    }
    let primary_released = ui.input(|input| input.pointer.primary_released());
    let drop_graph_position = primary_released
        .then(|| ui.input(|input| input.pointer.interact_pos()))
        .flatten()
        .map(|position| to_global.inverse() * position);

    let mut layout_changed = false;
    if let Ok(mut project) = project_lock.write() {
        apply_queued_node_edits(&mut project, edits, history_manager, node_editor_state);
        for edit in layout_edits {
            layout_changed |= apply_layout_edit(&mut project, edit);
        }
        if primary_released {
            let moved_node_ids = std::mem::take(&mut node_editor_state.moved_node_ids);
            if let Some(position) = drop_graph_position {
                layout_changed |=
                    reparent_nodes_at_drop(&mut project, comp_id, &moved_node_ids, position);
            }
        }
    }
    if selection_changed {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    if selection.last_selected_entity_id.is_some_and(|node_id| {
        project_lock.read().map_or(true, |project| {
            !project.nodes.contains_key(&node_id) && !project.clips.contains_key(&node_id)
        })
    }) {
        selection.selected_entities.clear();
        selection.last_selected_entity_id = None;
        selection.last_selected_track_id = None;
    }

    node_editor_state.layout_changed_during_drag |= layout_changed;
    if ui.input(|input| input.pointer.secondary_clicked()) {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }
    let created = handle_context_menu(
        ui,
        context_menu_state,
        project_lock,
        project_service,
        comp_id,
        &context_menu_exclusion_rects,
        to_global,
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
    let mut plan = AutoLayoutPlan::default();
    plan.composition_size = Some(composition.ui_size);

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
            let band = node_band_bounds(project, &clip.node_ids, &ranks, column_origin_x);
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
                column_origin_x,
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

        if let Some(bounds) = node_band_bounds(project, &track.node_ids, &ranks, column_origin_x) {
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
                column_origin_x,
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

    if let Some(bounds) = node_band_bounds(project, &composition.node_ids, &ranks, column_origin_x)
    {
        let direct_y = if composition.track_ids.is_empty() {
            composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP
        } else {
            composition_bottom + AUTO_LAYOUT_TRACK_GAP
        };
        let _ = layout_node_band(
            project,
            &composition.node_ids,
            &ranks,
            column_origin_x,
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

impl NodeBandBounds {
    fn width(self) -> f32 {
        self.max_x - self.min_x
    }
}

fn node_band_bounds(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    column_origin_x: f32,
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
    let column_step = estimated_node_width() + AUTO_LAYOUT_COLUMN_GAP;
    Some(NodeBandBounds {
        min_x: column_origin_x + min_rank as f32 * column_step,
        max_x: column_origin_x + max_rank as f32 * column_step + estimated_node_width(),
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
    column_origin_x: f32,
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

    let column_step = estimated_node_width() + AUTO_LAYOUT_COLUMN_GAP;
    let bounds = node_band_bounds(project, node_ids, ranks, column_origin_x)?;
    for (rank, group) in groups {
        let x = column_origin_x + rank as f32 * column_step;
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

fn estimated_node_size(project: &Project, node_id: Uuid) -> egui::Vec2 {
    let item = GraphItem::Node(node_id);
    let pin_rows = input_definitions(project, item)
        .len()
        .max(output_definitions(project, item).len());
    // These are conservative graph-space bounds for the complete rendered
    // card (header, pin rows and body controls), not just the body widget.
    // The extra pin term keeps plugin Nodes with unusually many ports safe.
    let base_height = match project.get_node(node_id).map(|node| &node.content) {
        Some(NodeContent::Generator(GeneratorContent::Text)) => 330.0,
        Some(NodeContent::Generator(GeneratorContent::Shape))
        | Some(NodeContent::Generator(GeneratorContent::SkSL)) => 300.0,
        Some(NodeContent::Generator(GeneratorContent::Solid)) => 240.0,
        Some(NodeContent::PluginOperation(_)) => 260.0,
        Some(NodeContent::Media(_) | NodeContent::Reference(_) | NodeContent::Merge) => 220.0,
        None => 220.0,
    };
    egui::vec2(
        estimated_node_width(),
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
        NodeEdit::SetOutputNode { owner, node_id } => {
            let container = match owner {
                PortOwner::Composition(id) => NodeContainer::Composition(id),
                PortOwner::Track(id) => NodeContainer::Track(id),
                PortOwner::Clip(id) => NodeContainer::Clip(id),
                PortOwner::Node(_) => return false,
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
            PortOwner::Clip(id) => project.get_clip_mut(id).is_some_and(|clip| {
                clip.update_property_or_keyframe(&key, time, value, None)
            }),
            PortOwner::Node(id) => project.get_node_mut(id).is_some_and(|node| {
                node.update_property_or_keyframe(&key, time, value, None)
            }),
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

fn handle_context_menu(
    ui: &mut egui::Ui,
    state: &mut Option<ContextMenuState>,
    project_lock: &Arc<RwLock<Project>>,
    project_service: &EditorService,
    comp_id: Uuid,
    context_menu_exclusion_rects: &[egui::Rect],
    to_global: egui::emath::TSTransform,
) -> bool {
    let canvas_size = project_lock
        .read()
        .ok()
        .and_then(|project| {
            project
                .get_composition(comp_id)
                .map(|composition| (composition.width, composition.height))
        })
        .unwrap_or((1920, 1080));
    let from_global = to_global.inverse();
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.hover_pos(),
            input.time,
        )
    });
    update_global_context_menu_for_secondary_click(
        state,
        secondary_clicked,
        pointer_position,
        ui.min_rect(),
        context_menu_exclusion_rects,
        to_global,
        open_time,
    );

    let mut should_close = false;
    type CreateAction = Box<dyn FnOnce(&mut Project) -> bool>;
    let mut action: Option<CreateAction> = None;

    if let Some(context) = state {
        let position = context.position;
        let graph_position = from_global * position;
        let response = egui::Area::new(egui::Id::new("node_ctx_menu"))
            .fixed_pos(position)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_max_width(220.0);
                    let search = ui.text_edit_singleline(&mut context.search_query);
                    if context.search_query.is_empty()
                        && !ui.memory(|memory| memory.has_focus(search.id))
                    {
                        search.request_focus();
                    }
                    ui.separator();

                    let query = context.search_query.to_lowercase();
                    if matches_query(&query, "text")
                        && qa_menu_button(ui, "Text", "node_editor.menu.create.text").clicked()
                    {
                        match project_service.create_text_node(
                            "Hello World",
                            library::editor::project_service::DEFAULT_TEXT_FONT,
                            canvas_size.0,
                            canvas_size.1,
                        ) {
                            Ok(node) => {
                                action = Some(Box::new(move |project| {
                                    create_prebuilt_node(project, graph_position, node, comp_id)
                                }));
                            }
                            Err(error) => log::error!("Cannot create Text Node: {error}"),
                        }
                        should_close = true;
                    }
                    if matches_query(&query, "solid color")
                        && qa_menu_button(ui, "Solid Color", "node_editor.menu.create.solid")
                            .clicked()
                    {
                        match project_service.create_solid_node(
                            library::model::frame::color::Color {
                                r: 255,
                                g: 0,
                                b: 0,
                                a: 255,
                            },
                            canvas_size.0,
                            canvas_size.1,
                        ) {
                            Ok(node) => {
                                action = Some(Box::new(move |project| {
                                    create_prebuilt_node(project, graph_position, node, comp_id)
                                }));
                            }
                            Err(error) => log::error!("Cannot create Solid Node: {error}"),
                        }
                        should_close = true;
                    }
                    if matches_query(&query, "shape rectangle")
                        && qa_menu_button(ui, "Shape (Rectangle)", "node_editor.menu.create.shape")
                            .clicked()
                    {
                        match project_service.create_shape_node(
                            library::editor::project_service::DEFAULT_SHAPE_PATH,
                            canvas_size.0,
                            canvas_size.1,
                            100,
                            100,
                        ) {
                            Ok(node) => {
                                action = Some(Box::new(move |project| {
                                    create_prebuilt_node(project, graph_position, node, comp_id)
                                }));
                            }
                            Err(error) => log::error!("Cannot create Shape Node: {error}"),
                        }
                        should_close = true;
                    }
                    if matches_query(&query, "fill style")
                        && qa_menu_button(ui, "Fill", "node_editor.menu.create.fill").clicked()
                    {
                        match project_service
                            .get_plugin_manager()
                            .create_style_operation_node("fill")
                        {
                            Ok(node) => {
                                action = Some(Box::new(move |project| {
                                    insert_prebuilt_graph(
                                        project,
                                        graph_position,
                                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                                        comp_id,
                                    )
                                }));
                            }
                            Err(error) => log::error!("Cannot create Fill Node: {error}"),
                        }
                        should_close = true;
                    }
                    if matches_query(&query, "stroke style")
                        && qa_menu_button(ui, "Stroke", "node_editor.menu.create.stroke").clicked()
                    {
                        match project_service
                            .get_plugin_manager()
                            .create_style_operation_node("stroke")
                        {
                            Ok(node) => {
                                action = Some(Box::new(move |project| {
                                    insert_prebuilt_graph(
                                        project,
                                        graph_position,
                                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                                        comp_id,
                                    )
                                }));
                            }
                            Err(error) => log::error!("Cannot create Stroke Node: {error}"),
                        }
                        should_close = true;
                    }
                    let plugin_manager = project_service.get_plugin_manager();
                    for (effector_id, effector_label) in
                        available_effector_menu_entries(plugin_manager.as_ref())
                    {
                        let search_label =
                            format!("effector {effector_label} {effector_id}").to_lowercase();
                        if !matches_query(&query, &search_label) {
                            continue;
                        }
                        let label = format!("Effector · {effector_label}");
                        let qa_id = format!("node_editor.menu.create.effector:{effector_id}");
                        if qa_menu_button(ui, &label, &qa_id).clicked() {
                            match plugin_manager.create_effector_operation_node(&effector_id) {
                                Ok(node) => {
                                    action = Some(Box::new(move |project| {
                                        insert_prebuilt_graph(
                                            project,
                                            graph_position,
                                            NodeGraphBundle::new(vec![node], Vec::new(), None),
                                            comp_id,
                                        )
                                    }));
                                }
                                Err(error) => log::error!(
                                    "Cannot create Effector Node {effector_id}: {error}"
                                ),
                            }
                            should_close = true;
                        }
                    }
                    let mut available_effects = plugin_manager.get_available_effects();
                    available_effects.sort_by(|left, right| {
                        left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0))
                    });
                    for (effect_id, effect_name, effect_category) in available_effects {
                        let search_label =
                            format!("effect {effect_name} {effect_id} {effect_category}")
                                .to_lowercase();
                        if !matches_query(&query, &search_label) {
                            continue;
                        }
                        let label = format!("Effect · {effect_name}");
                        let qa_id = format!("node_editor.menu.create.effect:{effect_id}");
                        if qa_menu_button(ui, &label, &qa_id).clicked() {
                            match plugin_manager.create_effect_operation_node(&effect_id) {
                                Ok(node) => {
                                    action = Some(Box::new(move |project| {
                                        insert_prebuilt_graph(
                                            project,
                                            graph_position,
                                            NodeGraphBundle::new(vec![node], Vec::new(), None),
                                            comp_id,
                                        )
                                    }));
                                }
                                Err(error) => {
                                    log::error!("Cannot create Effect Node {effect_id}: {error}")
                                }
                            }
                            should_close = true;
                        }
                    }
                    if matches_query(&query, "merge composite")
                        && qa_menu_button(ui, "Merge", "node_editor.menu.create.merge").clicked()
                    {
                        action = Some(Box::new(move |project| {
                            create_merge_node(project, graph_position, comp_id)
                        }));
                        should_close = true;
                    }
                    if matches_query(&query, "clip container")
                        && qa_menu_button(ui, "Container (Clip)", "node_editor.menu.create.clip")
                            .clicked()
                    {
                        action = Some(Box::new(move |project| {
                            create_clip_at_free_slot(project, graph_position, comp_id, "Clip")
                                .is_some()
                        }));
                        should_close = true;
                    }
                    if matches_query(&query, "track container")
                        && qa_menu_button(ui, "Container (Track)", "node_editor.menu.create.track")
                            .clicked()
                    {
                        action = Some(Box::new(move |project| {
                            create_track_at_free_slot(project, graph_position, comp_id, "Track")
                                .is_some()
                        }));
                        should_close = true;
                    }
                    if matches_query(&query, "container composition")
                        && qa_menu_button(
                            ui,
                            "Container (Composition)",
                            "node_editor.menu.create.composition",
                        )
                        .clicked()
                    {
                        action = Some(Box::new(move |project| {
                            create_composition_node(project, graph_position, comp_id)
                        }));
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
        if let Ok(mut project) = project_lock.write() {
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

fn matches_query(query: &str, label: &str) -> bool {
    query.is_empty() || label.contains(query)
}

fn qa_menu_button(ui: &mut egui::Ui, label: &str, id: &str) -> egui::Response {
    let response = ui.button(label);
    crate::qa::register_component(id, "node_editor_menu_item", response.rect);
    response
}

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

    let mut node = Node::new(
        "Container",
        NodeContent::Reference(library::model::ReferenceContent {
            target_id: nested_id,
            sync_global_time: false,
        }),
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
    create_prebuilt_node(
        project,
        position,
        Node::new("Merge", NodeContent::Merge),
        comp_id,
    )
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

/// Resolves the deepest visible container chrome under a Node drop. Expanded
/// headers and collapsed headers intentionally remain valid targets: a user
/// can reparent into a container without first expanding it. A collapsed
/// container's stored (hidden) body is not hit-testable because `rect()` is
/// reduced to the visible header height.
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

fn reparent_nodes_at_drop(
    project: &mut Project,
    composition_id: Uuid,
    node_ids: &HashSet<Uuid>,
    position: egui::Pos2,
) -> bool {
    let Some(destination) = node_container_at_position(project, composition_id, position)
    else {
        return false;
    };
    let mut candidate = project.clone();
    let mut changed = false;
    let mut node_ids = node_ids.iter().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    for node_id in node_ids {
        if candidate.find_node_container(node_id) == Some(destination) {
            continue;
        }
        match candidate.attach_node_to_container(destination, node_id) {
            Ok(()) => {
                if let Some(rect) = estimated_node_rect(&candidate, node_id) {
                    ensure_container_hierarchy_contains(&mut candidate, destination, rect);
                }
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
) {
    let owner = match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    };
    grow_container_to_rect(project, owner, node_rect);

    // Propagate the *grown child container rectangle*, not only the Node
    // rectangle. Each parent owns its own content margins; using the leaf
    // rectangle here made a resized Clip escape the Track by exactly those
    // margins after a multi-Node graph was inserted.
    let parent_track = match container {
        NodeContainer::Clip(clip_id) => project.find_track_for_clip(clip_id),
        NodeContainer::Track(track_id) => Some(track_id),
        NodeContainer::Composition(_) => None,
    };
    if let Some(track_id) = parent_track {
        if let NodeContainer::Clip(clip_id) = container {
            if let Some(clip) = project.get_clip(clip_id) {
                let clip_rect = container_rect(clip.ui_position, clip.ui_size);
                grow_container_to_rect(project, PortOwner::Track(track_id), clip_rect);
            }
        }
        if let Some(composition_id) = project.find_composition_for_track(track_id) {
            if let Some(track) = project.get_track(track_id) {
                let track_rect = container_rect(track.ui_position, track.ui_size);
                grow_container_to_rect(project, PortOwner::Composition(composition_id), track_rect);
            }
        }
    }
}

fn grow_container_to_rect(project: &mut Project, owner: PortOwner, rect: egui::Rect) {
    let Some(visual) = container_visual(project, owner) else {
        return;
    };
    let (right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) | PortOwner::Clip(_) => {
            (AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_BOTTOM)
        }
        PortOwner::Node(_) => return,
    };
    let size = [
        visual.size[0].max(rect.right() - visual.position[0] + right_margin),
        visual.size[1].max(rect.bottom() - visual.position[1] + bottom_margin),
    ];
    let _ = set_container_size(project, owner, size);
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
    use library::animation::EasingFunction;
    use library::model::project::{
        ProjectConnection, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
        SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
    };
    use library::model::property::{Keyframe, Property};

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

        let mut solid = Node::new("Solid", NodeContent::Generator(GeneratorContent::Solid));
        solid.properties.set(
            "color".to_string(),
            library::model::property::Property::constant(PropertyValue::Color(
                library::model::frame::color::Color {
                    r: 10,
                    g: 20,
                    b: 30,
                    a: 255,
                },
            )),
        );
        solid.ui_position = [450.0, 390.0];
        let solid_id = solid.id;
        project.add_node(solid);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), solid_id)
            .unwrap();

        let mut merge = Node::new("Merge", NodeContent::Merge);
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

    fn plugin_operation_component(node: &Node) -> Option<&str> {
        match &node.content {
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
        let mut text_node = Node::new("Tall Text", NodeContent::Generator(GeneratorContent::Text));
        text_node.properties.set(
            "text".to_string(),
            library::model::property::Property::constant(PropertyValue::String(
                "A deliberately tall text Node".to_string(),
            )),
        );
        text_node.properties.set(
            "font_family".to_string(),
            library::model::property::Property::constant(PropertyValue::String(
                "Arial".to_string(),
            )),
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
        let mut shape_node = Node::new(
            "Track Shape",
            NodeContent::Generator(GeneratorContent::Shape),
        );
        shape_node.properties.set(
            "path".to_string(),
            library::model::property::Property::constant(PropertyValue::String(
                "M 0 0 H 100 V 100 Z".to_string(),
            )),
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
        let mut root_merge = Node::new("Composition Merge", NodeContent::Merge);
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
            let _ = context.run(raw_input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
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
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::clone(&rendered_ports),
                    };
                    let mut style = SnarlStyle::default();
                    style.collapsible = Some(false);
                    style.min_scale = Some(0.05);
                    style.max_scale = Some(1.0);
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
            });
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
                    node.content,
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
                        == PortAddress::new(
                            PortOwner::Node(consumer_id),
                            MERGE_IMAGES_PORT,
                        )
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
                node.properties
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
                    node.properties
                        .get(definition.name())
                        .map(|property| property.evaluate_at(0.0)),
                    Some(definition.default_value().clone()),
                    "{component_id}.{} was not initialized by its descriptor factory",
                    definition.name(),
                );
            }
        }
        let transform = plugins.create_effector_operation_node("transform").unwrap();
        assert_eq!(
            transform
                .properties
                .get("target")
                .map(|property| property.evaluate_at(0.0)),
            Some(PropertyValue::String("Block".to_string()))
        );
        let opacity = plugins.create_effector_operation_node("opacity").unwrap();
        assert_eq!(
            opacity
                .properties
                .get("mode")
                .map(|property| property.evaluate_at(0.0)),
            Some(PropertyValue::String("Set".to_string()))
        );
        assert_eq!(
            opacity
                .properties
                .get("target")
                .map(|property| property.evaluate_at(0.0)),
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
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
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
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
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
                register_edge_component(
                    "node_editor.edge:overview-wire-transform-test".to_string(),
                    "explicit",
                    &from,
                    &to,
                    &ports,
                    None,
                    canvas,
                    pin_color(PortDataType::Image),
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
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports,
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
            ..base.clone()
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
            graph_item_inactive_reason(
                &project,
                GraphItem::Node(solid_id),
                0.5
            ),
            Some(GraphItemInactiveReason::OutsideClipRange)
        );
        assert_eq!(
            graph_item_inactive_reason(
                &project,
                GraphItem::Node(solid_id),
                1.0
            ),
            None
        );

        project.get_node_mut(solid_id).unwrap().enabled = false;
        assert_eq!(
            graph_item_inactive_reason(
                &project,
                GraphItem::Node(solid_id),
                1.0
            ),
            Some(GraphItemInactiveReason::Disabled)
        );
        // Disabled is the primary authored reason even when the Clip is also
        // outside its half-open active range.
        assert_eq!(
            graph_item_inactive_reason(
                &project,
                GraphItem::Node(solid_id),
                0.5
            ),
            Some(GraphItemInactiveReason::Disabled)
        );
        assert!(graph_item_inactive(
            &project,
            GraphItem::Node(solid_id),
            0.5
        ));
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
            .properties
            .set("animated".to_string(), animated.clone());

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
                .properties
                .get("animated")
                .unwrap()
                .evaluate_at(node_property_time(&project, solid_id, global_time)),
            PropertyValue::Number(OrderedFloat(42.5))
        );

        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Node(solid_id),
                key: "animated".into(),
                time: inspector_and_renderer_time,
                value: PropertyValue::Number(OrderedFloat(91.0)),
            }
        ));
        let clip_node_property = project
            .get_node(solid_id)
            .unwrap()
            .properties
            .get("animated")
            .unwrap();
        assert_eq!(
            clip_node_property.evaluate_at(inspector_and_renderer_time),
            PropertyValue::Number(OrderedFloat(91.0))
        );
        assert!(clip_node_property.has_keyframe_at(inspector_and_renderer_time, 0.001));
        assert!(!clip_node_property.has_keyframe_at(global_time, 0.001));

        let root_id = Uuid::from_u128(0x9_101);
        let mut root = Node::new("Root", NodeContent::Merge);
        root.id = root_id;
        root.properties.set("animated".to_string(), animated);
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
                key: "animated".into(),
                time: root_property_time,
                value: PropertyValue::Number(OrderedFloat(55.0)),
            }
        ));
        let root_property = project
            .get_node(root_id)
            .unwrap()
            .properties
            .get("animated")
            .unwrap();
        assert!(root_property.has_keyframe_at(global_time, 0.001));
        assert_eq!(
            root_property.evaluate_at(global_time),
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
        history.push_project_state(initial.clone());
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
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetProperty {
                owner: PortOwner::Node(node_id),
                key: "enabled".into(),
                time: 0.0,
                value: PropertyValue::Boolean(false),
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
                    "{node_id}: {rect:?}"
                );
                assert!(
                    rect.height() <= estimated.y * scale + 1.0,
                    "{node_id}: {rect:?}"
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
        let node = Node::new("New", NodeContent::Merge);
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
    fn reparent_drop_targets_include_visible_headers_but_exclude_collapsed_bodies() {
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

        // The collapsed header remains visible and is deliberately a drop
        // target, while its stored body is not.
        assert_eq!(
            node_container_at_position(&project, composition_id, clip_header),
            Some(NodeContainer::Clip(clip_id))
        );
        assert_eq!(
            node_container_at_position(&project, composition_id, hidden_clip_body),
            Some(NodeContainer::Track(track_id))
        );

        let node_id = project.get_clip(clip_id).unwrap().node_ids[0];
        assert!(reparent_nodes_at_drop(
            &mut project,
            composition_id,
            &HashSet::from([node_id]),
            hidden_clip_body,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        assert!(reparent_nodes_at_drop(
            &mut project,
            composition_id,
            &HashSet::from([node_id]),
            clip_header,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );

        let track_header = egui::pos2(500.0, 160.0);
        let hidden_track_body = egui::pos2(500.0, 400.0);
        project.get_track_mut(track_id).unwrap().ui_collapsed = true;
        assert_eq!(
            node_container_at_position(&project, composition_id, track_header),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            node_container_at_position(&project, composition_id, hidden_track_body),
            Some(NodeContainer::Composition(composition_id))
        );
        assert!(project.validate_containment().is_empty());
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
        let node = Node::new("Root Node", NodeContent::Merge);
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
        let node = Node::new(
            "Locally Placed Text",
            NodeContent::Generator(GeneratorContent::Text),
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
            let _ = context.run(raw_input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let (mut snarl, containers) = build_snarl(&project, composition_id);
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
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
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                    };
                    let mut style = SnarlStyle::default();
                    style.collapsible = Some(false);
                    style.max_scale = Some(1.0);
                    snarl.show(
                        &mut viewer,
                        &style,
                        egui::Id::new(("node-layout-test", composition_id)),
                        ui,
                    );
                });
            });
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
