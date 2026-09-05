//! Production Node Editor surface for one bounded Module graph.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui_snarl::{InPinId, OutPinId, Snarl};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, GraphFrame, InteractionOptions, ItemId,
    NodeDescriptor, PortDescriptor, PortDirection as SurfacePortDirection, PortOwner,
    ReconnectEndpoint, TypeKey, WireDescriptor,
};

use super::context_menu::show_module_create_menu;
use super::interface::port_interface_actions;
use super::viewer::{ModuleNodeViewer, ModuleSurfaceCapture};
use super::*;
use crate::ui::panels::node_editor::{
    node_editor_details_visible, node_editor_navigation_config, node_editor_snarl_style_for,
    NODE_EDITOR_MAX_SCALE, NODE_EDITOR_MIN_SCALE, PORT_ROW_HEIGHT,
};
use crate::ui::viewport::{ViewportController, ViewportState};

impl ViewportState for NodeEditorState {
    fn canvas_state(&self) -> pan_zoom_ui::CanvasState {
        self.canvas
    }

    fn set_canvas_state(&mut self, state: pan_zoom_ui::CanvasState) {
        self.canvas = state;
    }
}

pub(super) fn fit_module_document_canvas(
    definition: &ModuleDefinition,
    viewport: egui::Rect,
) -> Option<pan_zoom_ui::CanvasState> {
    let bounds = definition.graph.nodes.values().fold(None, |bounds, node| {
        let size = super::layout::sanitized_size(node.ui_size);
        let node_rect = egui::Rect::from_min_size(
            egui::pos2(node.ui_position[0], node.ui_position[1]),
            egui::vec2(size[0], size[1]),
        );
        Some(bounds.map_or(node_rect, |bounds: egui::Rect| bounds.union(node_rect)))
    })?;
    let mut fitted = pan_zoom_ui::fit_canvas(
        viewport,
        bounds.size(),
        egui::Vec2::splat(28.0),
        NODE_EDITOR_MIN_SCALE,
        NODE_EDITOR_MAX_SCALE,
    )?;
    fitted.state.pan -= bounds.min.to_vec2() * fitted.state.zoom;
    Some(fitted.state)
}

pub(super) fn show_module_document(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    palette: &library::model::authoring::ProjectPalette,
    state: &mut NodeEditorState,
    plugins: &PluginManager,
    property_context: ModulePropertyContext,
) -> Vec<ModuleEditorAction> {
    let viewport = ui.available_rect_before_wrap();
    if !viewport.is_positive() {
        return Vec::new();
    }

    release_finished_direct_gesture(ui, state);
    retain_press_time_transform(ui, state, viewport);
    let locked_transform = state
        .surface_interaction
        .locked_transform()
        .or(state.direct_gesture_transform);
    if locked_transform.is_none() {
        let mut handled_pan = false;
        let mut controller = ViewportController::new(
            ui,
            ui.id().with("module_graph_viewport"),
            Some(egui::Key::Space),
        )
        .with_config(node_editor_navigation_config())
        .with_direct_manipulation_passthrough(true)
        .with_screen_origin(viewport.min);
        let _ = controller.interact_with_rect(viewport, state, &mut handled_pan);
    }
    let authoritative_transform =
        locked_transform.unwrap_or_else(|| node_editor_canvas_transform(viewport, state.canvas));

    let mut snarl = build_module_snarl(definition, &state.node_drag_offsets);
    let capture = Arc::new(Mutex::new(ModuleSurfaceCapture::default()));
    let mut actions = Vec::new();
    let mut transform = authoritative_transform;
    let mut canvas_clip = viewport;
    {
        let mut viewer = ModuleNodeViewer {
            definition,
            palette,
            plugins,
            property_context,
            selected_nodes: &state.selected_nodes,
            actions: &mut actions,
            canvas_transform: authoritative_transform,
            to_global: &mut transform,
            canvas_clip: &mut canvas_clip,
            capture: Arc::clone(&capture),
        };
        let style = node_editor_snarl_style_for(ui.style());
        snarl.show(&mut viewer, &style, ("node_editor", definition.id), ui);
    }

    let capture = capture
        .lock()
        .map(|mut capture| std::mem::take(&mut *capture))
        .unwrap_or_default();
    let projection = ModuleSurfaceProjection::new(
        definition,
        capture,
        viewport,
        transform,
        state,
        ui.ctx().pointer_latest_pos(),
    );
    let options = if node_editor_details_visible(transform.scaling) {
        InteractionOptions {
            delete: true,
            connect: true,
            disconnect: true,
            ..InteractionOptions::SELECTION_AND_MOVE
        }
    } else {
        InteractionOptions::OVERVIEW_SELECTION
    };
    let outputs = Editor::interact(
        ui,
        &projection.frame(),
        &mut state.surface_interaction,
        options,
        projection.pointer_owned,
    );
    actions.extend(translate_surface_outputs(definition, outputs, state));
    actions.extend(port_interface_actions(
        ui,
        definition,
        &projection.port_visuals,
        transform,
        viewport,
    ));
    register_wire_interaction_qa(&projection);
    if let Some((request, graph_position)) = show_module_create_menu(
        ui,
        state,
        plugins,
        definition,
        viewport,
        transform,
        &projection.node_rects,
    ) {
        actions.push(ModuleEditorAction::CreateNode {
            request,
            graph_position,
        });
    }

    register_canvas_qa(
        definition,
        viewport,
        transform,
        projection.pointer_owned,
        options.connect,
    );
    actions
}

fn register_wire_interaction_qa(projection: &ModuleSurfaceProjection<'_>) {
    if !crate::qa::is_enabled() {
        return;
    }
    let scale = projection.transform.scaling.abs().max(f32::EPSILON);
    let frame = projection.frame();
    for wire in &projection.wires {
        let geometry = wire.curve.interaction_geometry(scale);
        if let Some(body_center) = Editor::wire_selection_target(&frame, &wire.id) {
            let body_rect = interaction_rect(
                body_center,
                geometry.body_hit_radius() * scale,
                projection.viewport,
            );
            crate::qa::register_component_with_metadata(
                format!("node_editor.connection:{}", wire.id),
                "node_editor_connection",
                body_rect,
                wire.editable,
                Some(serde_json::json!({
                    "connection_id": wire.id,
                    "from_node_id": wire.from.address.node_id,
                    "from_port": wire.from.address.port,
                    "to_node_id": wire.to.address.node_id,
                    "to_port": wire.to.address.port,
                    "interaction_geometry": "node-editor-ui",
                })),
            );
        }

        if !wire.editable || !projection.selection.contains(&ItemId::Wire(wire.id)) {
            continue;
        }
        for (endpoint, name) in [
            (ReconnectEndpoint::Source, "source"),
            (ReconnectEndpoint::Target, "target"),
        ] {
            let handle_rect = interaction_rect(
                projection.transform * geometry.reconnect_handle(endpoint),
                geometry.reconnect_handle_hit_radius() * scale,
                projection.viewport,
            );
            crate::qa::register_component_with_metadata(
                format!("node_editor.connection_handle:{}:{name}", wire.id),
                "node_editor_connection_handle",
                handle_rect,
                true,
                Some(serde_json::json!({
                    "connection_id": wire.id,
                    "endpoint": name,
                    "interaction_geometry": "node-editor-ui",
                })),
            );
        }
    }
}

fn interaction_rect(center: egui::Pos2, radius: f32, viewport: egui::Rect) -> egui::Rect {
    egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0)).intersect(viewport)
}

fn node_editor_canvas_transform(
    viewport: egui::Rect,
    canvas: pan_zoom_ui::CanvasState,
) -> egui::emath::TSTransform {
    egui::emath::TSTransform::new(viewport.min.to_vec2() + canvas.pan, canvas.zoom.x)
}

fn release_finished_direct_gesture(ui: &egui::Ui, state: &mut NodeEditorState) {
    let primary_down = ui.input(|input| input.pointer.primary_down());
    if !primary_down {
        state.direct_gesture_transform = None;
    }
}

fn retain_press_time_transform(ui: &egui::Ui, state: &mut NodeEditorState, viewport: egui::Rect) {
    let (primary_pressed, space_down, pointer) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.key_down(egui::Key::Space),
            input.pointer.interact_pos(),
        )
    });
    if primary_pressed && !space_down && pointer.is_some_and(|pointer| viewport.contains(pointer)) {
        state.direct_gesture_transform = Some(node_editor_canvas_transform(viewport, state.canvas));
    }
}

pub(super) fn build_module_snarl(
    definition: &ModuleDefinition,
    offsets: &HashMap<Uuid, egui::Vec2>,
) -> Snarl<Uuid> {
    let mut snarl = Snarl::new();
    let mut ids = HashMap::new();
    let mut nodes = definition.graph.nodes.values().collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        let position = egui::pos2(node.ui_position[0], node.ui_position[1])
            + offsets.get(&node.id).copied().unwrap_or_default();
        let id = if node.ui_collapsed {
            snarl.insert_node_collapsed(position, node.id)
        } else {
            snarl.insert_node(position, node.id)
        };
        ids.insert(node.id, id);
    }

    for connection in &definition.graph.connections {
        let (Some(&from_node), Some(&to_node)) = (
            ids.get(&connection.from.node_id),
            ids.get(&connection.to.node_id),
        ) else {
            continue;
        };
        let Some(from_index) = port_index(
            definition,
            connection.from.node_id,
            PortDirection::Output,
            &connection.from.port,
        ) else {
            continue;
        };
        let Some(to_index) = port_index(
            definition,
            connection.to.node_id,
            PortDirection::Input,
            &connection.to.port,
        ) else {
            continue;
        };
        snarl.connect(
            OutPinId {
                node: from_node,
                output: from_index,
            },
            InPinId {
                node: to_node,
                input: to_index,
            },
        );
    }
    snarl
}

pub(super) fn port_index(
    definition: &ModuleDefinition,
    node_id: Uuid,
    direction: PortDirection,
    key: &str,
) -> Option<usize> {
    let node = definition.graph.nodes.get(&node_id)?;
    document_port_contract(definition, node)
        .ok()?
        .ports
        .iter()
        .filter(|port| port.direction == direction)
        .position(|port| port.key == key)
}

struct ModuleSurfaceProjection<'a> {
    nodes: Vec<NodeDescriptor<'a, Uuid, Uuid>>,
    ports: Vec<PortDescriptor<'static, Uuid, ModuleEditorPortId, Uuid, PortDataType>>,
    wires: Vec<WireDescriptor<ModuleEditorPortId, ModuleConnectionId>>,
    selection_order: Vec<ItemId<Uuid, Uuid, ModuleConnectionId>>,
    selection: Vec<ItemId<Uuid, Uuid, ModuleConnectionId>>,
    primary: Option<ItemId<Uuid, Uuid, ModuleConnectionId>>,
    port_visuals: Vec<PortVisual>,
    node_rects: Vec<egui::Rect>,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
    pointer_owned: bool,
}

impl<'a> ModuleSurfaceProjection<'a> {
    fn new(
        definition: &'a ModuleDefinition,
        capture: ModuleSurfaceCapture,
        viewport: egui::Rect,
        transform: egui::emath::TSTransform,
        state: &NodeEditorState,
        pointer_position: Option<egui::Pos2>,
    ) -> Self {
        let pointer_owned = capture.owns_pointer(pointer_position);
        let mut order = capture.selection_order;
        let mut missing = definition
            .graph
            .nodes
            .keys()
            .filter(|id| !order.contains(id))
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        order.extend(missing);

        let nodes = order
            .iter()
            .filter_map(|node_id| {
                let node = definition.graph.nodes.get(node_id)?;
                let rect = *capture.node_rects.get(node_id)?;
                let header_rect = capture
                    .header_rects
                    .get(node_id)
                    .copied()
                    .unwrap_or_else(|| {
                        egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(rect.width(), PORT_ROW_HEIGHT.min(rect.height())),
                        )
                    });
                Some(NodeDescriptor {
                    id: *node_id,
                    title: node.name.as_str(),
                    rect,
                    header_rect,
                    parent: None,
                    enabled: node.enabled,
                })
            })
            .collect::<Vec<_>>();
        let mut port_visuals = capture.ports.into_values().collect::<Vec<_>>();
        port_visuals.sort_by(|left, right| {
            left.id
                .address
                .node_id
                .cmp(&right.id.address.node_id)
                .then_with(|| {
                    direction_rank(left.id.direction).cmp(&direction_rank(right.id.direction))
                })
                .then_with(|| left.id.address.port.cmp(&right.id.address.port))
        });
        let ports = port_visuals
            .iter()
            .map(|port| PortDescriptor {
                id: port.id.clone(),
                owner: PortOwner::Node(port.id.address.node_id),
                label: "",
                center: port.center,
                direction: match port.id.direction {
                    PortDirection::Input => SurfacePortDirection::Input,
                    PortDirection::Output => SurfacePortDirection::Output,
                },
                type_key: TypeKey::new(port.data_type),
                connectable: module_port_is_connectable(definition, port),
            })
            .collect::<Vec<_>>();
        let centers = port_visuals
            .iter()
            .map(|port| (port.id.clone(), port.center))
            .collect::<HashMap<_, _>>();
        let wires = definition
            .graph
            .connections
            .iter()
            .filter_map(|connection| {
                let from_id = ModuleEditorPortId {
                    address: connection.from.clone(),
                    direction: PortDirection::Output,
                };
                let to_id = ModuleEditorPortId {
                    address: connection.to.clone(),
                    direction: PortDirection::Input,
                };
                let from = *centers.get(&from_id)?;
                let to = *centers.get(&to_id)?;
                let handle = ((to.x - from.x).abs() * 0.5).max(48.0);
                Some(WireDescriptor {
                    id: connection.id,
                    from: from_id,
                    to: to_id,
                    curve: CubicBezier::new(
                        from,
                        from + egui::vec2(handle, 0.0),
                        to - egui::vec2(handle, 0.0),
                        to,
                    ),
                    editable: true,
                })
            })
            .collect();
        let selection_order = order.into_iter().map(ItemId::Node).collect();
        let mut selection = state
            .selected_nodes
            .iter()
            .copied()
            .map(ItemId::Node)
            .collect::<Vec<_>>();
        if let Some(connection) = state.selected_connection {
            selection.push(ItemId::Wire(connection));
        }
        let primary = state
            .primary_node
            .map(ItemId::Node)
            .or_else(|| state.selected_connection.map(ItemId::Wire));
        let node_rects = nodes.iter().map(|node| node.rect).collect();

        Self {
            nodes,
            ports,
            wires,
            selection_order,
            selection,
            primary,
            port_visuals,
            node_rects,
            viewport,
            transform,
            pointer_owned,
        }
    }

    fn frame(
        &self,
    ) -> GraphFrame<'_, Uuid, ModuleEditorPortId, ModuleConnectionId, Uuid, PortDataType> {
        GraphFrame {
            viewport: self.viewport,
            transform: self.transform,
            nodes: &self.nodes,
            ports: &self.ports,
            wires: &self.wires,
            groups: &[],
            ports_compatible: |source, target| target.accepts(*source),
            selection_order: &self.selection_order,
            selection: AuthoritativeSelection {
                items: &self.selection,
                primary: self.primary,
            },
        }
    }
}

pub(super) fn module_port_is_connectable(definition: &ModuleDefinition, port: &PortVisual) -> bool {
    match port.id.direction {
        PortDirection::Output => true,
        PortDirection::Input => !definition
            .input_port_ownership(&port.id.address)
            .is_externally_driven(),
    }
}

const fn direction_rank(direction: PortDirection) -> u8 {
    match direction {
        PortDirection::Input => 0,
        PortDirection::Output => 1,
    }
}

fn register_canvas_qa(
    definition: &ModuleDefinition,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
    pointer_owned_by_node_control: bool,
    connect_enabled: bool,
) {
    crate::qa::register_component_with_metadata(
        "node_editor.canvas",
        "node_editor_canvas",
        viewport,
        true,
        Some(serde_json::json!({
            "document_kind": "module_definition",
            "module_definition_id": definition.id,
            "module_node_count": definition.graph.nodes.len(),
            "module_connection_count": definition.graph.connections.len(),
            "scale": transform.scaling,
            "translation": {
                "x": transform.translation.x,
                "y": transform.translation.y,
            },
            "pan": {
                "x": transform.translation.x - viewport.min.x,
                "y": transform.translation.y - viewport.min.y,
            },
            "viewport_controller": "shared",
            "production_surface": "egui_snarl",
            "timeline_graph_expansion": false,
            "pointer_owned_by_node_control": pointer_owned_by_node_control,
            "connect_enabled": connect_enabled,
        })),
    );
}
