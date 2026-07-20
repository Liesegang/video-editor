use crate::state::context_types::{NodeEditorEditableWire, NodeEditorState};
use crate::ui::widgets::searchable_context_menu::{
    searchable_menu_click_is_outside, searchable_popup_placement, show_searchable_items_with_qa,
};
use eframe::egui;
use library::model::project::{PortDataType, PortOwner};
use library::model::Project;
use library::plugin::PluginManager;
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    blend_mode_label, blend_mode_qa_key, connection_supports_authored_blend,
    create_operation_node_for_request, editable_wire_is_current, editable_wire_stable_key,
    non_selectable_label, qa_container_key, wire_order_menu_state, wire_order_qa_metadata,
    wire_splice_menu_items, NodeEdit, QueuedNodeEdit, AUTHORED_BLEND_MODES,
};

pub(in crate::ui::panels::node_editor) fn show_wire_context_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    project: &Project,
    plugin_manager: &PluginManager,
    composition_id: Uuid,
    to_global: egui::emath::TSTransform,
) -> Option<QueuedNodeEdit> {
    let target = state.wire_context_menu.as_ref()?.target;
    if let NodeEditorEditableWire::OutputBinding {
        owner,
        node_id,
        data_type,
    } = target
    {
        return show_output_binding_wire_context_menu(
            ui, state, project, owner, node_id, data_type,
        );
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
    let searchable_menu_id = format!(
        "node_editor_wire_insert_menu:{connection_id}:{}",
        context.open_time.to_bits()
    );
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
                        &searchable_menu_id,
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
                            "Layer Order · Back to Front {} / {}",
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
        && searchable_menu_click_is_outside(ui.ctx(), &searchable_menu_id, response.response.rect)
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
    data_type: PortDataType,
) -> Option<QueuedNodeEdit> {
    let target = NodeEditorEditableWire::OutputBinding {
        owner,
        node_id,
        data_type,
    };
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
                let output_type =
                    crate::ui::panels::node_editor::container_output_type_key(data_type)
                        .unwrap_or("unsupported");
                non_selectable_label(ui, format!("{output_type} Container Output Binding"))
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
                        "output_type": output_type,
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
            "output_type": crate::ui::panels::node_editor::container_output_type_key(data_type),
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
