use super::ProjectNodeViewer;
use crate::ui::panels::node_editor::*;
use crate::ui::panels::time_context::{time_source_state, TimeSourceState};
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};
use crate::ui::widgets::property_mode::{
    property_for_mode, property_mode_control, toggled_keyframe_property, PropertyModeAction,
};
use crate::ui::widgets::searchable_context_menu::show_searchable_items_with_qa;
use eframe::egui::{self, Color32};
use egui_phosphor::regular as icons;
use library::model::project::{PortOwner, TIME_PORT};
use library::model::property::{Property, PropertyDefinition, PropertyUiType, PropertyValue};
use ordered_float::OrderedFloat;
use uuid::Uuid;

mod vector;

fn merge_row_reorder_edit(row: &MergeLayerRow, new_index: usize) -> NodeEdit {
    row.structural_child.map_or(
        NodeEdit::ReorderConnection {
            connection_id: row.connection_id,
            new_order: new_index as i64,
        },
        |binding| NodeEdit::ReorderStructuralChild {
            container: binding.container,
            child: binding.owner,
            new_index,
        },
    )
}

impl ProjectNodeViewer<'_> {
    pub(super) fn show_merge_input_slot(
        &mut self,
        merge_id: Uuid,
        slot: &MergeInputSlot,
        ui: &mut egui::Ui,
    ) -> Option<Uuid> {
        let MergeInputSlotRole::Connected(row) = &slot.role else {
            if let MergeInputSlotRole::Vacant(kind) = &slot.role {
                let kind = *kind;
                let layer_count = merge_layer_rows(self.project, merge_id).len();
                let canonical_insertion_slot = kind.vacant_canonical_index(layer_count);
                let response = non_selectable_label(
                    ui,
                    egui::RichText::new(format!(
                        "{} Connect {}{}",
                        icons::PLUS,
                        kind.display_name(),
                        if kind == NativeVariadicMergeKind::Image {
                            " as Back"
                        } else {
                            ""
                        }
                    ))
                        .small()
                        .weak(),
                )
                .on_hover_text(match kind {
                    NativeVariadicMergeKind::Image => {
                        "Vacant variadic input; a new Image wire stays on this bottom row and is inserted behind every existing layer"
                    }
                    NativeVariadicMergeKind::Sound => {
                        "Vacant variadic input; a new Sound wire is appended in canonical top-to-bottom order"
                    }
                });
                register_merge_layer_component(
                    format!("node_editor.merge_layer.vacant:{merge_id}"),
                    "node_editor_merge_layer_vacant_input",
                    response.rect,
                    true,
                    *self.to_global,
                    *self.canvas_clip,
                    serde_json::json!({
                        "merge_id": merge_id,
                        "action": "connect",
                        "merge_kind": kind.qa_key(),
                        "port": kind.input_port(),
                        "variadic": true,
                        "canonical_insertion_slot": canonical_insertion_slot,
                        "visual_slot": layer_count,
                        "insertion_semantics": kind.vacant_insertion_semantics(),
                        "canonical_order_semantics": kind.canonical_order_semantics(),
                        "visual_order_semantics": kind.visual_order_semantics(),
                    }),
                );
                if layer_count == 0 {
                    register_merge_layer_component(
                        format!("node_editor.merge_layers.empty:{merge_id}"),
                        "node_editor_merge_layers_empty",
                        response.rect,
                        false,
                        *self.to_global,
                        *self.canvas_clip,
                        serde_json::json!({
                            "merge_id": merge_id,
                            "layer_count": 0,
                            "merge_kind": kind.qa_key(),
                            "canonical_order_semantics": kind.canonical_order_semantics(),
                            "visual_order_semantics": kind.visual_order_semantics(),
                            "order_semantics": kind.canonical_order_semantics(),
                        }),
                    );
                }
            }
            return None;
        };

        let to_global = *self.to_global;
        let canvas_clip = *self.canvas_clip;
        let active_target = self
            .merge_layer_reorder
            .as_ref()
            .and_then(|gesture| (gesture.merge_id == merge_id).then_some(gesture.target_index));
        let target_highlight = active_target.flatten() == Some(row.canonical_index);
        let mut selected_blend = None;
        let mut requested_order = None;
        let mut drag_response = None;
        let mut up_response = None;
        let mut down_response = None;
        let mut up_target = None;
        let mut down_target = None;
        let row_response = egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(
                5,
                crate::ui::panels::node_editor::types::MERGE_LAYER_VERTICAL_MARGIN,
            ))
            .corner_radius(4)
            .fill(if target_highlight {
                Color32::from_rgba_premultiplied(120, 154, 230, 72)
            } else {
                Color32::from_black_alpha(28)
            })
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(
                    MERGE_BODY_WIDTH,
                    crate::ui::panels::node_editor::types::MERGE_LAYER_BODY_HEIGHT,
                ));
                ui.horizontal(|ui| {
                    let handle = ui
                        .add_enabled(
                            row.reorder_min_index < row.reorder_max_index,
                            egui::Label::new(
                                egui::RichText::new(icons::DOTS_SIX_VERTICAL).strong(),
                            )
                            .sense(egui::Sense::drag()),
                        )
                        .on_hover_cursor(egui::CursorIcon::Grab)
                        .on_hover_text("Drag vertically to reorder this input wire");
                    drag_response = Some(handle);
                    non_selectable_label(
                        ui,
                        egui::RichText::new(match row.kind {
                            NativeVariadicMergeKind::Image => {
                                format!("Front {} / {}", row.visual_index + 1, row.layer_count)
                            }
                            NativeVariadicMergeKind::Sound => {
                                format!("Input {} / {}", row.canonical_index + 1, row.layer_count)
                            }
                        })
                        .small()
                        .strong(),
                    );
                    bounded_non_selectable_label(
                        ui,
                        row.source_label.clone(),
                        94.0,
                        egui::Align::LEFT,
                    )
                    .on_hover_text(format!("{} · {}", row.source_label, row.source.port));
                    if row.kind == NativeVariadicMergeKind::Image {
                        let combo = ui.add_enabled_ui(row.authored_blend_available, |ui| {
                            egui::ComboBox::from_id_salt((
                                "merge_layer_authored_blend",
                                merge_id,
                                row.connection_id,
                            ))
                            .selected_text(blend_mode_label(row.authored_blend_mode))
                            .width(126.0)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show_ui(ui, |ui| {
                                let mut items =
                                    blend_mode_searchable_items(row.authored_blend_mode);
                                for item in &mut items {
                                    let blend_mode = item.value;
                                    let selected = !item.enabled;
                                    item.qa_id = Some(format!(
                                        "node_editor.merge_layer.blend.{}:{merge_id}:{}",
                                        blend_mode_qa_key(blend_mode),
                                        row.connection_id
                                    ));
                                    item.qa_metadata =
                                        Some(row.qa_metadata(Some(serde_json::json!({
                                            "action": "set_authored_blend",
                                            "blend_mode": blend_mode_qa_key(blend_mode),
                                            "blend_group": blend_mode.group().qa_key(),
                                            "selected": selected,
                                            "coordinate_space": "screen_points",
                                        }))));
                                }
                                if let Some(blend_mode) = show_searchable_items_with_qa(
                                    ui,
                                    &format!(
                                        "merge_layer_blend_menu:{merge_id}:{}",
                                        row.connection_id
                                    ),
                                    Some(&format!(
                                        "node_editor.merge_layer.blend_search:{merge_id}:{}",
                                        row.connection_id
                                    )),
                                    &items,
                                ) {
                                    selected_blend = Some(blend_mode);
                                }
                            })
                            .response
                        });
                        register_merge_layer_component(
                            format!(
                                "node_editor.merge_layer.blend_select:{merge_id}:{}",
                                row.connection_id
                            ),
                            "node_editor_merge_layer_blend_select",
                            combo.inner.rect,
                            combo.inner.enabled(),
                            to_global,
                            canvas_clip,
                            row.qa_metadata(Some(serde_json::json!({
                                "action": "open_authored_blend",
                            }))),
                        );
                    } else {
                        bounded_non_selectable_label(
                            ui,
                            format!("Canonical {}", row.canonical_index + 1),
                            92.0,
                            egui::Align::Center,
                        )
                        .on_hover_text("Sound inputs are mixed in canonical top-to-bottom order");
                    }

                    let up_index = match row.kind {
                        NativeVariadicMergeKind::Image => row.canonical_index.checked_add(1),
                        NativeVariadicMergeKind::Sound => row.canonical_index.checked_sub(1),
                    }
                    .filter(|index| {
                        *index >= row.reorder_min_index && *index <= row.reorder_max_index
                    });
                    up_target = up_index;
                    let response = ui
                        .add_enabled(up_index.is_some(), egui::Button::new(icons::ARROW_UP))
                        .on_hover_text(match row.kind {
                            NativeVariadicMergeKind::Image => "Move one layer toward the front",
                            NativeVariadicMergeKind::Sound => "Move one Sound input earlier",
                        });
                    if response.clicked() {
                        requested_order = up_index;
                    }
                    up_response = Some(response);
                    let down_index = match row.kind {
                        NativeVariadicMergeKind::Image => row.canonical_index.checked_sub(1),
                        NativeVariadicMergeKind::Sound => row.canonical_index.checked_add(1),
                    }
                    .filter(|index| {
                        *index >= row.reorder_min_index && *index <= row.reorder_max_index
                    });
                    down_target = down_index;
                    let response = ui
                        .add_enabled(down_index.is_some(), egui::Button::new(icons::ARROW_DOWN))
                        .on_hover_text(match row.kind {
                            NativeVariadicMergeKind::Image => "Move one layer toward the back",
                            NativeVariadicMergeKind::Sound => "Move one Sound input later",
                        });
                    if response.clicked() {
                        requested_order = down_index;
                    }
                    down_response = Some(response);
                });
            })
            .response;

        let Some(drag_response) = drag_response else {
            return Some(row.connection_id);
        };

        if drag_response.drag_started() {
            *self.merge_layer_reorder = Some(NodeEditorMergeLayerReorderGesture {
                merge_id,
                connection_id: row.connection_id,
                start_index: row.canonical_index,
                target_index: Some(row.canonical_index),
                layer_count: row.layer_count,
                reorder_min_index: row.reorder_min_index,
                reorder_max_index: row.reorder_max_index,
                row_rects: vec![egui::Rect::NOTHING; row.layer_count],
                canvas_transform: to_global,
                finished: false,
            });
        }
        if let Some(gesture) = self.merge_layer_reorder.as_mut().filter(|gesture| {
            gesture.merge_id == merge_id && gesture.layer_count == row.layer_count
        }) {
            if row.canonical_index >= gesture.reorder_min_index
                && row.canonical_index <= gesture.reorder_max_index
            {
                if let Some(rect) = gesture.row_rects.get_mut(row.canonical_index) {
                    *rect = row_response.rect;
                }
            }
        }
        let owns_drag = self.merge_layer_reorder.as_ref().is_some_and(|gesture| {
            gesture.merge_id == merge_id && gesture.connection_id == row.connection_id
        });
        if owns_drag {
            let pointer = ui
                .input(|input| input.pointer.interact_pos())
                .map(|position| to_global.inverse() * position);
            let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
            let target_index = self.merge_layer_reorder.as_ref().and_then(|gesture| {
                let measured = gesture
                    .row_rects
                    .iter()
                    .enumerate()
                    .filter(|(_, rect)| rect.is_positive())
                    .collect::<Vec<_>>();
                let bounds = measured
                    .iter()
                    .fold(egui::Rect::NOTHING, |bounds, (_, rect)| {
                        bounds.union(**rect)
                    })
                    .expand2(egui::vec2(24.0, 6.0));
                let pointer = pointer.filter(|position| bounds.contains(*position))?;
                measured
                    .into_iter()
                    .min_by(|left, right| {
                        (left.1.center().y - pointer.y)
                            .abs()
                            .total_cmp(&(right.1.center().y - pointer.y).abs())
                    })
                    .map(|(index, _)| index)
            });
            if let Some(gesture) = self.merge_layer_reorder.as_mut() {
                gesture.target_index = (!escape).then_some(target_index).flatten();
            }
            if escape || drag_response.drag_stopped() {
                if let Some(gesture) = self.merge_layer_reorder.as_mut() {
                    let changed_target = gesture
                        .target_index
                        .filter(|target| *target != gesture.start_index);
                    if let Some(target_index) = (!escape).then_some(changed_target).flatten() {
                        self.edits
                            .push(QueuedNodeEdit::Atomic(merge_row_reorder_edit(
                                row,
                                target_index,
                            )));
                    }
                    gesture.finished = true;
                }
            }
            ui.ctx().request_repaint();
        }

        let active = self.merge_layer_reorder.as_ref().filter(|gesture| {
            gesture.merge_id == merge_id && gesture.connection_id == row.connection_id
        });
        register_merge_layer_component(
            format!("node_editor.merge_layer:{merge_id}:{}", row.connection_id),
            "node_editor_merge_layer",
            row_response.rect,
            true,
            to_global,
            canvas_clip,
            row.qa_metadata(Some(serde_json::json!({
                "action": "physical_reorder_drop_target",
                "drop_target_index": row.canonical_index,
                "drop_target_canonical_index": row.canonical_index,
                "drag_active": active.is_some(),
                "current_drop_target_index": active.and_then(|gesture| gesture.target_index),
                "gesture_start_index": active.map(|gesture| gesture.start_index),
            }))),
        );
        let (up_direction, down_direction) = match row.kind {
            NativeVariadicMergeKind::Image => ("front", "back"),
            NativeVariadicMergeKind::Sound => ("earlier", "later"),
        };
        for (direction, response, target_index) in [
            (up_direction, up_response, up_target),
            (down_direction, down_response, down_target),
        ] {
            if let Some(response) = response {
                register_merge_layer_component(
                    format!(
                        "node_editor.merge_layer.order_{direction}:{merge_id}:{}",
                        row.connection_id
                    ),
                    "node_editor_merge_layer_order_button",
                    response.rect,
                    response.enabled(),
                    to_global,
                    canvas_clip,
                    row.qa_metadata(Some(serde_json::json!({
                        "action": "reorder",
                        "direction": direction,
                        "target_canonical_index": target_index,
                        "target_back_to_front_index": (row.kind == NativeVariadicMergeKind::Image)
                            .then_some(target_index)
                            .flatten(),
                    }))),
                );
            }
        }
        register_merge_layer_component(
            format!(
                "node_editor.merge_layer.drag_handle:{merge_id}:{}",
                row.connection_id
            ),
            "node_editor_merge_layer_drag_handle",
            drag_response.rect,
            row.reorder_min_index < row.reorder_max_index,
            to_global,
            canvas_clip,
            row.qa_metadata(Some(serde_json::json!({
                "action": "physical_reorder",
                "gesture": "primary_vertical_drag",
                "drop_target_index": active.and_then(|gesture| gesture.target_index),
                "invalid_drop_cancels": true,
            }))),
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
                .push(QueuedNodeEdit::Atomic(merge_row_reorder_edit(
                    row, new_order,
                )));
        }
        Some(row.connection_id)
    }

    pub(super) fn queue_continuous_edit(
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

    pub(super) fn show_node_input_row(
        &mut self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        definition: &PinDefinition,
        property_key: &str,
        property_definition: Option<&PropertyDefinition>,
        connected: bool,
    ) {
        if definition.key == TIME_PORT {
            if let Some(state) = time_source_state(self.project, PortOwner::Node(node_id)) {
                self.show_node_time_source_row(ui, node_id, definition, connected, &state);
                return;
            }
        }

        let property_time = node_property_time(self.project, node_id, self.current_time);
        let authored_property = self
            .project
            .get_node(node_id)
            .and_then(|node| node.properties().get(property_key))
            .cloned();
        let evaluated = authored_property.as_ref().map(|property| {
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
        let mode_value = value
            .clone()
            .or_else(|| {
                authored_property
                    .as_ref()
                    .and_then(Property::value)
                    .cloned()
            })
            .or_else(|| property_definition.map(|definition| definition.default_value().clone()));
        let current_value_metadata = value
            .as_ref()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        let row = ui.horizontal(|ui| {
            bounded_non_selectable_label(ui, definition.name.clone(), 72.0, egui::Align::LEFT);
            let mode_qa_id = format!("node_editor.property_mode.node:{node_id}:{property_key}");
            let mode_action =
                property_mode_control(ui, &mode_qa_id, authored_property.as_ref(), property_time);
            let replacement = match (mode_action, authored_property.as_ref(), mode_value.clone()) {
                (Some(PropertyModeAction::SetMode(mode)), current, Some(value)) => {
                    property_for_mode(current, mode, value, property_time).ok()
                }
                (Some(PropertyModeAction::ToggleKeyframe), Some(current), Some(value)) => {
                    toggled_keyframe_property(current, value, property_time)
                }
                _ => None,
            };
            if let Some(property) = replacement {
                self.edits
                    .push(QueuedNodeEdit::Atomic(NodeEdit::ReplaceProperty {
                        node_id,
                        key: property_key.to_string(),
                        property,
                    }));
            }
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
                    egui::RichText::new(icons::MINUS)
                        .small()
                        .color(Color32::from_gray(105)),
                )
                .on_hover_text("No value");
                return None;
            };
            let (changed, continuous, finished, control_kind, response, vector_components) =
                match &mut value {
                    PropertyValue::Number(number) => {
                        let response = if let Some(config) =
                            property_definition.and_then(FloatDragValueConfig::from_definition)
                        {
                            ui.add_sized(
                                [74.0, PORT_ROW_HEIGHT - 2.0],
                                config.widget(&mut number.0),
                            )
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
                            Vec::new(),
                        )
                    }
                    PropertyValue::Integer(integer) => {
                        let config = property_definition.and_then(|definition| {
                            IntegerDragValueConfig::from_ui_type(definition.ui_type())
                        });
                        let response = if let Some(config) = config {
                            ui.add_sized([74.0, PORT_ROW_HEIGHT - 2.0], config.widget(integer))
                        } else {
                            ui.add_sized(
                                [74.0, PORT_ROW_HEIGHT - 2.0],
                                egui::DragValue::new(integer),
                            )
                        };
                        (
                            response.changed(),
                            true,
                            continuous_response_finished(ui, &response),
                            "integer",
                            response,
                            Vec::new(),
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
                                Vec::new(),
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
                                Vec::new(),
                            )
                        }
                    }
                    PropertyValue::Boolean(boolean) => {
                        let response = ui.checkbox(boolean, "");
                        (
                            response.changed(),
                            false,
                            false,
                            "boolean",
                            response,
                            Vec::new(),
                        )
                    }
                    PropertyValue::Color(color) => {
                        let mut edited =
                            Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
                        let (response, popup_closed) =
                            continuous_color_edit_button(ui, &mut edited);
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
                            Vec::new(),
                        )
                    }
                    PropertyValue::Vec2(_) | PropertyValue::Vec3(_) | PropertyValue::Vec4(_) => {
                        let Some(rendered) = vector::render(ui, property_definition, &mut value)
                        else {
                            log::error!("Vector property {property_key} could not be rendered");
                            let response = ui
                                .colored_label(ui.visuals().error_fg_color, "Invalid vector value");
                            return Some((
                                response,
                                "invalid_vector",
                                serde_json::Value::Null,
                                Vec::new(),
                            ));
                        };
                        (
                            rendered.changed,
                            true,
                            rendered.finished,
                            rendered.control_kind,
                            rendered.response,
                            rendered.axes,
                        )
                    }
                    PropertyValue::Array(_) | PropertyValue::Map(_) => {
                        let response = non_selectable_label(
                            ui,
                            egui::RichText::new("complex")
                                .small()
                                .color(Color32::from_gray(125)),
                        );
                        (
                            false,
                            false,
                            false,
                            "complex_readonly",
                            response,
                            Vec::new(),
                        )
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
            Some((response, control_kind, qa_value, vector_components))
        });
        let (response, control_kind, enabled, value, vector_components) = match row.inner {
            Some((response, control_kind, value, vector_components)) => {
                let enabled = response.enabled();
                (response, control_kind, enabled, value, vector_components)
            }
            None => (
                row.response,
                if connected { "linked" } else { "missing" },
                false,
                current_value_metadata,
                Vec::new(),
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
        vector::register_components(
            self,
            node_id,
            property_key,
            definition,
            property_definition,
            connected,
            property_time,
            vector_components,
        );
    }

    fn show_node_time_source_row(
        &self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        definition: &PinDefinition,
        connected: bool,
        state: &TimeSourceState,
    ) {
        let presentation = state.presentation(self.project);
        let row = ui.horizontal(|ui| {
            bounded_non_selectable_label(ui, definition.name.clone(), 72.0, egui::Align::LEFT);
            ui.add_sized(
                [164.0, PORT_ROW_HEIGHT - 2.0],
                egui::Label::new(
                    egui::RichText::new(&presentation.label)
                        .small()
                        .color(Color32::from_gray(145)),
                )
                .selectable(false)
                .truncate(),
            )
            .on_hover_text(&presentation.tooltip)
        });

        let component_id = format!("node_editor.time_source.node:{node_id}");
        let unclipped_rect = *self.to_global * row.inner.rect;
        let rect = clipped_qa_rect(unclipped_rect, *self.canvas_clip);
        let mut metadata = state.qa_metadata(PortOwner::Node(node_id));
        if let Some(metadata) = metadata.as_object_mut() {
            metadata.insert("label".to_string(), presentation.label.into());
            metadata.insert("tooltip".to_string(), presentation.tooltip.into());
            metadata.insert("connected".to_string(), connected.into());
            metadata.insert(
                "unclipped_rect".to_string(),
                qa_rect_metadata(unclipped_rect),
            );
            metadata.insert("visible_in_canvas".to_string(), rect.is_positive().into());
        }
        #[cfg(test)]
        {
            capture_test_rect(&component_id, rect);
            capture_test_metadata(&component_id, &metadata);
        }
        crate::qa::register_component_with_metadata(
            component_id,
            "node_time_source",
            rect,
            true,
            Some(metadata),
        );
    }

    pub(super) fn show_container_body(&mut self, owner: PortOwner, ui: &mut egui::Ui) {
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

    pub(super) fn edit_string_property(
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
