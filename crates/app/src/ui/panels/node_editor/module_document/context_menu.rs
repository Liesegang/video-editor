use super::*;
use crate::state::node_editor::ModuleCreateMenuState;
use crate::ui::widgets::searchable_context_menu::{
    register_searchable_popup_qa, searchable_menu_click_is_outside, searchable_popup_placement,
    show_searchable_items_with_qa, show_searchable_popup_frame,
};

#[derive(Clone)]
struct NodeNameDraft {
    source: String,
    text: String,
    frame: u64,
}

fn node_name_editor_id(definition: &ModuleDefinition, node: &Node) -> egui::Id {
    egui::Id::new(("module.node_name", definition.id, node.id))
}

pub(super) fn show_node_name(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    node: &Node,
) -> Option<ModuleEditorAction> {
    let id = node_name_editor_id(definition, node);
    let frame = ui.ctx().cumulative_frame_nr();
    let mut draft = ui
        .data(|data| data.get_temp::<NodeNameDraft>(id))
        .filter(|draft| draft.source == node.name && frame <= draft.frame.saturating_add(1))
        .unwrap_or_else(|| NodeNameDraft {
            source: node.name.clone(),
            text: node.name.clone(),
            frame,
        });
    let edit = crate::ui::widgets::name_editor::name_editor(
        ui,
        id,
        &mut draft.text,
        &node.name,
        220.0,
        |name| library::core::render_plan::validate_module_node_name(definition, node.id, name),
    );
    crate::qa::register_component_with_metadata(
        format!("node_editor.node_menu:{}:name", node.id),
        "node_name_editor",
        edit.response.rect,
        edit.response.enabled(),
        Some(serde_json::json!({
            "node_id": node.id,
            "source": node.name,
            "draft": draft.text,
            "validation_error": edit.error,
        })),
    );
    draft.frame = frame;
    ui.data_mut(|data| data.insert_temp(id, draft));
    if edit.cancelled || edit.value.is_some() {
        ui.close();
    }
    edit.value.map(|name| ModuleEditorAction::SetNodeState {
        node_id: node.id,
        name,
        enabled: node.enabled,
        bypassed: node.bypassed,
    })
}

pub(super) fn show_module_node_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    definition: &ModuleDefinition,
) -> Option<ModuleEditorAction> {
    let context = state.node_menu.clone()?;
    let Some(node) = definition.graph.nodes.get(&context.node_id) else {
        state.node_menu = None;
        return None;
    };
    let popup_id = ui.make_persistent_id(("node_editor_node_context_menu", definition.id, node.id));
    let mut open = true;
    let popup = show_node_popup(ui, popup_id, context.position, &mut open, |ui| {
        node_menu_contents(ui, definition, &state.selected_nodes, node)
    });
    let mut action = None;
    if let Some(popup) = popup {
        crate::qa::register_component_with_metadata(
            format!("node_editor.node_menu:{}", node.id),
            "node_context_menu",
            popup.response.rect,
            true,
            Some(serde_json::json!({
                "node_id": node.id,
                "close_behavior": "outside_click",
            })),
        );
        action = popup.inner;
    }
    if !open {
        state.node_menu = None;
        ui.data_mut(|data| data.remove::<NodeNameDraft>(node_name_editor_id(definition, node)));
    }
    action
}

fn show_node_popup<R>(
    ui: &mut egui::Ui,
    id: egui::Id,
    position: egui::Pos2,
    open: &mut bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<egui::InnerResponse<R>> {
    egui::Popup::new(id, ui.ctx().clone(), position, ui.layer_id())
        .open_bool(open)
        .kind(egui::PopupKind::Menu)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .width(220.0)
        .show(contents)
}

fn node_menu_contents(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    selected_nodes: &std::collections::HashSet<Uuid>,
    node: &Node,
) -> Option<ModuleEditorAction> {
    let node_id = node.id;
    let is_output = is_module_output_node(definition, node_id);
    let is_protected = definition.is_protected_host_boundary_node(node_id);
    if let Some(action) = super::clipboard::menu_actions(
        ui,
        definition,
        selected_nodes,
        Some(node_id),
        egui::pos2(node.ui_position[0] + 32.0, node.ui_position[1] + 32.0),
    ) {
        ui.close();
        return Some(action);
    }
    ui.separator();
    const OUTPUT_STATE_REASON: &str =
        "Module Output is a required render terminal and cannot be disabled or bypassed.";
    const OUTPUT_DELETE_REASON: &str =
        "Module Output is a required render terminal and cannot be deleted.";
    const HOST_BOUNDARY_STATE_REASON: &str = "Transition A/B/Progress boundaries are supplied by the Timeline and cannot be disabled or bypassed.";
    const HOST_BOUNDARY_DELETE_REASON: &str = "Transition A/B/Progress boundaries are required by the host contract and cannot be deleted.";
    if let Some(action) = show_node_name(ui, definition, node) {
        ui.close();
        return Some(action);
    }
    let mut enabled = node.enabled;
    let enabled_response = ui.add_enabled(
        !is_output && !is_protected,
        egui::Checkbox::new(&mut enabled, "Enabled"),
    );
    if enabled_response.changed() {
        ui.close();
        return Some(ModuleEditorAction::SetNodeState {
            node_id,
            name: node.name.clone(),
            enabled,
            bypassed: node.bypassed,
        });
    }
    if is_output {
        register_output_control(node_id, "enabled", &enabled_response, OUTPUT_STATE_REASON);
        enabled_response.on_hover_text(OUTPUT_STATE_REASON);
    } else if is_protected {
        register_host_boundary_control(
            node_id,
            "enabled",
            &enabled_response,
            HOST_BOUNDARY_STATE_REASON,
        );
        enabled_response.on_hover_text(HOST_BOUNDARY_STATE_REASON);
    }
    let mut bypassed = node.bypassed;
    let bypass_response = ui.add_enabled(
        !is_output && !is_protected && node.supports_bypass(),
        egui::Checkbox::new(&mut bypassed, "Bypass"),
    );
    if bypass_response.changed() {
        ui.close();
        return Some(ModuleEditorAction::SetNodeState {
            node_id,
            name: node.name.clone(),
            enabled: node.enabled,
            bypassed,
        });
    }
    if is_output {
        register_output_control(node_id, "bypass", &bypass_response, OUTPUT_STATE_REASON);
        bypass_response.on_hover_text(OUTPUT_STATE_REASON);
    } else if is_protected {
        register_host_boundary_control(
            node_id,
            "bypass",
            &bypass_response,
            HOST_BOUNDARY_STATE_REASON,
        );
        bypass_response.on_hover_text(HOST_BOUNDARY_STATE_REASON);
    }
    ui.separator();
    let delete_response = ui.add_enabled(
        !is_output && !is_protected,
        egui::Button::new(format!("{} Delete Node", egui_phosphor::regular::TRASH))
            .shortcut_text("Del"),
    );
    crate::qa::register_component_with_metadata(
        format!("node_editor.node_menu:{node_id}:delete"),
        "node_menu_action",
        delete_response.rect,
        delete_response.enabled(),
        Some(serde_json::json!({"node_id": node_id, "action": "delete"})),
    );
    if delete_response.clicked() {
        ui.close();
        return Some(ModuleEditorAction::DeleteNodes(vec![node_id]));
    }
    if is_output {
        register_output_control(node_id, "delete", &delete_response, OUTPUT_DELETE_REASON);
        delete_response.on_hover_text(OUTPUT_DELETE_REASON);
    } else if is_protected {
        register_host_boundary_control(
            node_id,
            "delete",
            &delete_response,
            HOST_BOUNDARY_DELETE_REASON,
        );
        delete_response.on_hover_text(HOST_BOUNDARY_DELETE_REASON);
    }
    None
}

fn register_output_control(
    node_id: Uuid,
    action: &str,
    response: &egui::Response,
    disabled_reason: &str,
) {
    crate::qa::register_component_with_metadata(
        format!("node_editor.output_control:{node_id}:{action}"),
        "node_editor_output_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "node_id": node_id,
            "action": action,
            "module_output": true,
            "disabled_reason": disabled_reason,
        })),
    );
}

fn register_host_boundary_control(
    node_id: Uuid,
    action: &str,
    response: &egui::Response,
    disabled_reason: &str,
) {
    crate::qa::register_component_with_metadata(
        format!("node_editor.host_boundary_control:{node_id}:{action}"),
        "node_editor_host_boundary_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "node_id": node_id,
            "action": action,
            "host_boundary": true,
            "disabled_reason": disabled_reason,
        })),
    );
}

pub(super) fn show_module_create_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    plugins: &PluginManager,
    definition: &ModuleDefinition,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
    node_rects: &[egui::Rect],
) -> Option<ModuleEditorAction> {
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.interact_pos(),
            input.time,
        )
    });
    update_for_secondary_click(
        &mut state.create_menu,
        secondary_clicked && !egui::Popup::is_any_open(ui.ctx()),
        pointer_position,
        viewport,
        node_rects,
        transform,
        open_time,
    );

    let mut selected = None;
    let mut should_close = false;
    if let Some(context) = state.create_menu.as_ref() {
        let position = context.position;
        let graph_position = visible_creation_position(position, viewport, transform);
        let popup =
            searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
        let menu_id = format!("node_editor_add_menu:{}", context.open_time.to_bits());
        let response = egui::Area::new(egui::Id::new("node_editor_context_menu"))
            .order(egui::Order::Foreground)
            .pivot(popup.pivot)
            .fixed_pos(popup.area_anchor)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                show_searchable_popup_frame(ui, popup, |ui| {
                    if let Some(action) = super::clipboard::menu_actions(
                        ui,
                        definition,
                        &state.selected_nodes,
                        None,
                        graph_position,
                    ) {
                        selected = Some(action);
                        should_close = true;
                    }
                    ui.separator();
                    let items =
                        super::menu::module_node_menu_items(plugins, &definition.host_contract);
                    if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &menu_id,
                        Some("node_editor.menu.search"),
                        &items,
                    ) {
                        selected = Some(ModuleEditorAction::CreateNode {
                            request,
                            graph_position,
                        });
                        should_close = true;
                    }
                })
            });
        let root_rect = response.inner.response.rect;
        register_searchable_popup_qa("node_editor.menu.root", position, popup, root_rect);
        if ui.input(|input| input.pointer.any_click())
            && ui.input(|input| input.time) - context.open_time > 0.2
            && searchable_menu_click_is_outside(ui.ctx(), &menu_id, root_rect)
        {
            should_close = true;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
    }
    if should_close {
        state.create_menu = None;
    }
    selected
}

pub(super) fn show_module_wire_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
) -> Option<ModuleConnectionId> {
    let context = state.wire_menu.as_ref()?;
    let position = context.position;
    let connection_id = context.connection_id;
    let open_time = context.open_time;
    let popup =
        searchable_popup_placement(position, egui::vec2(252.0, 88.0), ui.ctx().content_rect());
    let menu_id = format!("node_editor_wire_menu:{connection_id}");
    let mut disconnect = false;
    let response = egui::Area::new(egui::Id::new("node_editor_wire_context_menu"))
        .order(egui::Order::Foreground)
        .pivot(popup.pivot)
        .fixed_pos(popup.area_anchor)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            show_searchable_popup_frame(ui, popup, |ui| {
                let button = ui.add(
                    egui::Button::new(format!("{} Disconnect", egui_phosphor::regular::PLUG))
                        .shortcut_text("Del"),
                );
                crate::qa::register_component_with_metadata(
                    "node_editor.wire_menu.disconnect",
                    "menu_item",
                    button.rect,
                    true,
                    Some(serde_json::json!({"connection_id": connection_id})),
                );
                disconnect = button.clicked();
                ui.separator();
                ui.weak("Ctrl + Right-drag   Cut Links");
                ui.weak("Alt + Right-drag    Lazy Connect");
            })
        });
    let root_rect = response.inner.response.rect;
    register_searchable_popup_qa("node_editor.wire_menu", position, popup, root_rect);
    let clicked_outside = ui
        .input(|input| input.pointer.any_click() && input.time - open_time > 0.2)
        && searchable_menu_click_is_outside(ui.ctx(), &menu_id, root_rect);
    if disconnect || clicked_outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        state.wire_menu = None;
    }
    disconnect.then_some(connection_id)
}

/// Place a new Node around the invocation point while keeping its initial
/// controls reachable. The mature Snarl surface can measure a more precise
/// size on the next frame; this conservative footprint prevents a context
/// menu near an edge from creating every output port off-screen.
pub(super) fn visible_creation_position(
    pointer: egui::Pos2,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
) -> egui::Pos2 {
    const SCREEN_MARGIN: f32 = 12.0;
    const CREATED_NODE_WIDTH: f32 = 420.0;
    const CREATED_NODE_HEIGHT: f32 = 220.0;

    let scale = transform.scaling.abs().max(f32::EPSILON);
    let available =
        (viewport.size() - egui::Vec2::splat(SCREEN_MARGIN * 2.0)).max(egui::Vec2::ZERO);
    let footprint = (egui::vec2(CREATED_NODE_WIDTH, CREATED_NODE_HEIGHT) * scale).min(available);
    let minimum = viewport.min + egui::Vec2::splat(SCREEN_MARGIN);
    let maximum = viewport.max - egui::Vec2::splat(SCREEN_MARGIN) - footprint;
    let desired = pointer - footprint * 0.5;
    let screen_position = egui::pos2(
        desired.x.clamp(minimum.x, maximum.x.max(minimum.x)),
        desired.y.clamp(minimum.y, maximum.y.max(minimum.y)),
    );
    transform.inverse() * screen_position
}

fn update_for_secondary_click(
    state: &mut Option<ModuleCreateMenuState>,
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
        *state = None;
        return;
    }
    *state = Some(ModuleCreateMenuState::new(position, open_time));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup_frame(
        context: &egui::Context,
        frame: u64,
        open: &mut bool,
        draft: &mut String,
        events: Vec<egui::Event>,
    ) -> egui::Rect {
        let mut edit_rect = egui::Rect::NOTHING;
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    drop(show_node_popup(
                        ui,
                        egui::Id::new("node-menu-popup-test"),
                        egui::pos2(80.0, 70.0),
                        open,
                        |ui| {
                            edit_rect = ui.text_edit_singleline(draft).rect;
                        },
                    ));
                });
            },
        ));
        edit_rect
    }

    fn pointer_button(
        position: egui::Pos2,
        button: egui::PointerButton,
        pressed: bool,
    ) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn node_popup_keeps_inline_name_clicks_open() {
        let context = egui::Context::default();
        let mut open = true;
        let mut draft = "Node".to_string();
        popup_frame(&context, 0, &mut open, &mut draft, Vec::new());
        let edit = popup_frame(&context, 1, &mut open, &mut draft, Vec::new());
        assert!(edit.is_positive());
        let point = edit.center();
        let press_edit = popup_frame(
            &context,
            2,
            &mut open,
            &mut draft,
            vec![
                egui::Event::PointerMoved(point),
                pointer_button(point, egui::PointerButton::Primary, true),
            ],
        );
        assert!(
            press_edit.contains(point),
            "press target {point:?} moved outside {press_edit:?}"
        );
        assert!(open, "popup closed on the inline TextEdit press");
        let release_edit = popup_frame(
            &context,
            3,
            &mut open,
            &mut draft,
            vec![pointer_button(point, egui::PointerButton::Primary, false)],
        );
        assert!(
            open,
            "an inline TextEdit click at {point:?} must not close the Node menu; release rect {release_edit:?}"
        );
    }

    #[test]
    fn node_popup_survives_the_secondary_release_that_opens_it() {
        let context = egui::Context::default();
        let point = egui::pos2(320.0, 240.0);
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                time: Some(0.0),
                events: vec![
                    egui::Event::PointerMoved(point),
                    pointer_button(point, egui::PointerButton::Secondary, true),
                ],
                ..Default::default()
            },
            |_| {},
        ));
        let mut open = true;
        let mut draft = "Node".to_string();
        popup_frame(
            &context,
            1,
            &mut open,
            &mut draft,
            vec![pointer_button(point, egui::PointerButton::Secondary, false)],
        );
        assert!(
            open,
            "the opening secondary release must not dismiss the popup"
        );
    }

    #[test]
    fn node_surface_prevents_the_blank_canvas_menu() {
        let mut state = None;
        update_for_secondary_click(
            &mut state,
            true,
            Some(egui::pos2(120.0, 120.0)),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(300.0, 300.0)),
            &[egui::Rect::from_min_max(
                egui::pos2(100.0, 100.0),
                egui::pos2(200.0, 200.0),
            )],
            egui::emath::TSTransform::IDENTITY,
            1.0,
        );
        assert!(state.is_none());
    }

    #[test]
    fn creation_near_an_edge_keeps_a_conservative_node_footprint_visible() {
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(600.0, 400.0));
        let transform = egui::emath::TSTransform::new(egui::vec2(100.0, 50.0), 0.5);

        let graph_position =
            visible_creation_position(egui::pos2(695.0, 445.0), viewport, transform);
        let screen_position = transform * graph_position;
        let footprint = egui::vec2(420.0, 220.0) * transform.scaling;

        assert!(screen_position.x >= viewport.left() + 12.0);
        assert!(screen_position.y >= viewport.top() + 12.0);
        assert!(screen_position.x + footprint.x <= viewport.right() - 12.0 + f32::EPSILON);
        assert!(screen_position.y + footprint.y <= viewport.bottom() - 12.0 + f32::EPSILON);
    }
}
