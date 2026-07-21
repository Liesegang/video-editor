use egui::Ui;
use library::model::frame::color::Color;
use library::model::property::{Property, PropertyDefinition, PropertyUiType, PropertyValue};
use ordered_float::OrderedFloat;

use super::evaluation::{evaluate_property_map, render_evaluation_issues};
use super::property_authoring::{render_property_authoring, PropertyAction};
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};

mod vector;

pub struct PropertyRenderContext<'a> {
    pub available_fonts: &'a [String],
    pub in_grid: bool,
    pub current_time: f64,
    /// Stable owner/target scope used by coordinate-driven QA. Examples:
    /// `node:<uuid>` and `node:<uuid>.effector:<instance-uuid>`.
    pub qa_scope: String,
}

// Helper function to handle common property events
fn handle_prop_response(
    actions: &mut Vec<PropertyAction>,
    response: &egui::Response,
    name: &str,
    new_value: Option<PropertyValue>,
    default_value: &PropertyValue,
) {
    if response.changed() {
        if let Some(val) = new_value {
            actions.push(PropertyAction::Update(name.to_string(), val));
        }
    }
    if response.middle_clicked() {
        actions.push(PropertyAction::Update(
            name.to_string(),
            default_value.clone(),
        ));
        actions.push(PropertyAction::Commit);
    }
    if response.drag_stopped() || response.lost_focus() {
        actions.push(PropertyAction::Commit);
    }
}

// Helper function to render generic property rows
// Returns a list of actions to transform the state
pub fn render_property_rows<G, GP>(
    ui: &mut Ui,
    properties: &[PropertyDefinition],
    get_value: G,
    get_property: GP,
    context: &PropertyRenderContext,
) -> Vec<PropertyAction>
where
    G: Fn(&str) -> Option<PropertyValue>,
    GP: Fn(&str) -> Option<Property>,
{
    let mut actions = Vec::new();

    for prop_def in properties {
        let property = get_property(prop_def.name());
        let evaluated_value = get_value(prop_def.name());
        let authored_value = property
            .as_ref()
            .filter(|property| property.evaluator == "expression")
            .and_then(Property::value)
            .cloned()
            .or(evaluated_value);
        let mode_value = authored_value
            .clone()
            .unwrap_or_else(|| prop_def.default_value().clone());
        actions.extend(render_property_authoring(
            ui,
            prop_def,
            property.as_ref(),
            &mode_value,
            context.current_time,
            &context.qa_scope,
            context.in_grid,
        ));

        // 2. Render Input Column
        match prop_def.ui_type() {
            PropertyUiType::Float { .. } => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Float property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt
                    .and_then(|v| {
                        v.get_as::<f64>()
                            .or_else(|| v.get_as::<f32>().map(|f| f as f64))
                    })
                    .unwrap_or(prop_def.default_value().get_as::<f64>().unwrap_or(0.0));

                let mut val_mut = current_val;
                let Some(config) = FloatDragValueConfig::from_definition(prop_def) else {
                    log::error!(
                        "Float property '{}' has incompatible UI metadata",
                        prop_def.name()
                    );
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Invalid Float metadata for {}", prop_def.label()),
                    );
                    if context.in_grid {
                        ui.end_row();
                    }
                    continue;
                };
                let response = ui.add(config.widget(&mut val_mut));
                register_property_control(
                    context,
                    prop_def,
                    "float",
                    &response,
                    serde_json::json!(current_val),
                );

                handle_prop_response(
                    &mut actions,
                    &response,
                    prop_def.name(),
                    Some(PropertyValue::Number(OrderedFloat(val_mut))),
                    prop_def.default_value(),
                );

                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Integer { .. } => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Integer property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt
                    .and_then(|v| v.get_as::<i64>())
                    .unwrap_or(prop_def.default_value().get_as::<i64>().unwrap_or(0));

                let mut val_mut = current_val;
                let Some(config) = IntegerDragValueConfig::from_ui_type(prop_def.ui_type()) else {
                    log::error!(
                        "Integer property '{}' has incompatible UI metadata",
                        prop_def.name()
                    );
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Invalid Integer metadata for {}", prop_def.label()),
                    );
                    if context.in_grid {
                        ui.end_row();
                    }
                    continue;
                };
                let response = ui.add(config.widget(&mut val_mut));
                register_property_control(
                    context,
                    prop_def,
                    "integer",
                    &response,
                    serde_json::json!(current_val),
                );

                handle_prop_response(
                    &mut actions,
                    &response,
                    prop_def.name(),
                    Some(PropertyValue::Integer(val_mut)),
                    prop_def.default_value(),
                );

                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Color => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Color property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt.and_then(|v| v.get_as::<Color>()).unwrap_or(
                    prop_def
                        .default_value()
                        .get_as::<Color>()
                        .unwrap_or_default(),
                );

                let mut color32 = egui::Color32::from_rgba_premultiplied(
                    current_val.r,
                    current_val.g,
                    current_val.b,
                    current_val.a,
                );

                ui.horizontal(|ui| {
                    let response = ui.color_edit_button_srgba(&mut color32);
                    register_property_control(
                        context,
                        prop_def,
                        "color",
                        &response,
                        serde_json::json!({
                            "r": current_val.r,
                            "g": current_val.g,
                            "b": current_val.b,
                            "a": current_val.a,
                        }),
                    );

                    let changed = response.changed();

                    // Logic to detect when the popup was open and is now closed (panel close -> commit)
                    let popup_id = response.id.with("popup");
                    let is_open = egui::Popup::is_id_open(ui.ctx(), popup_id);

                    if is_open {
                        ui.data_mut(|d| d.insert_temp(popup_id, true)); // Mark as "was open"
                    } else {
                        // Not open now. Was it open?
                        let was_open = ui.data(|d| d.get_temp(popup_id).unwrap_or(false));
                        if was_open {
                            // It just closed (or we just noticed it closed).
                            // Trigger commit if we tracked changes, or just trigger commit to be safe.
                            // Since we don't track "dirty" here easily across frames without more data,
                            // we assume if it was open and now closed, we should commit.
                            // Actually, standard behavior is usually sufficient if we just commit on close.

                            // However, we only want to commit if we actually changed something?
                            // User said: "commit on panel close".

                            actions.push(PropertyAction::Commit);
                            ui.data_mut(|d| d.remove_temp::<bool>(popup_id));
                        }
                    }

                    if changed {
                        let new_color = Color {
                            r: color32.r(),
                            g: color32.g(),
                            b: color32.b(),
                            a: color32.a(),
                        };
                        actions.push(PropertyAction::Update(
                            prop_def.name().to_string(),
                            PropertyValue::Color(new_color),
                        ));
                    }
                    // Interpolation Mode UI
                    let prop_meta = get_property(prop_def.name());
                    if let Some(prop) = prop_meta {
                        if prop.evaluator == "keyframe" {
                            let current_mode = prop
                                .properties
                                .get("interpolation")
                                .and_then(|v| v.get_as::<String>())
                                .unwrap_or_else(|| "linear".to_string());

                            let mut mode = current_mode.clone();
                            egui::ComboBox::from_id_salt(format!("interp_{}", prop_def.name()))
                                .selected_text(if mode == "hsv" { "HSV" } else { "RGB" })
                                .width(60.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut mode, "linear".to_string(), "RGB");
                                    ui.selectable_value(&mut mode, "hsv".to_string(), "HSV");
                                });

                            if mode != current_mode {
                                actions.push(PropertyAction::SetAttribute(
                                    prop_def.name().to_string(),
                                    "interpolation".to_string(),
                                    PropertyValue::String(mode),
                                ));
                            }
                        }
                    }
                });

                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Bool => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Bool property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt
                    .and_then(|v| v.get_as::<bool>())
                    .unwrap_or(prop_def.default_value().get_as().unwrap_or(false));
                let mut val_mut = current_val;
                let response = ui.checkbox(&mut val_mut, "");
                register_property_control(
                    context,
                    prop_def,
                    "boolean",
                    &response,
                    serde_json::json!(current_val),
                );

                handle_prop_response(
                    &mut actions,
                    &response,
                    prop_def.name(),
                    Some(PropertyValue::Boolean(val_mut)),
                    prop_def.default_value(),
                );

                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Dropdown { options } => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Dropdown property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt
                    .and_then(|v| v.get_as::<String>())
                    .unwrap_or(prop_def.default_value().get_as().unwrap_or_default());

                let mut selected = current_val.clone();
                let response_inner =
                    egui::ComboBox::from_id_salt(format!("combo_{}", prop_def.name()))
                        .selected_text(&selected)
                        .show_ui(ui, |ui| {
                            for opt in options {
                                ui.selectable_value(&mut selected, opt.clone(), opt.clone());
                            }
                        });
                register_property_control(
                    context,
                    prop_def,
                    "dropdown",
                    &response_inner.response,
                    serde_json::json!(current_val),
                );

                // Dropdown specific handling for standard response
                let changed = selected != current_val;
                // Synthesize response for handle_prop_response if needed, or just call manually
                // ComboBox returns InnerResponse, header response is in response_inner.response

                if changed {
                    actions.push(PropertyAction::Update(
                        prop_def.name().to_string(),
                        PropertyValue::String(selected),
                    ));
                    actions.push(PropertyAction::Commit);
                }

                // Middle click on the collapsed combo box
                if response_inner.response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name().to_string(),
                        prop_def.default_value().clone(),
                    ));
                    actions.push(PropertyAction::Commit);
                }

                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Font => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Font property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt
                    .and_then(|v| v.get_as::<String>())
                    .unwrap_or(prop_def.default_value().get_as().unwrap_or_default());

                let mut selected = current_val.clone();
                let response =
                    egui::ComboBox::from_id_salt(format!("combo_font_{}", prop_def.name()))
                        .selected_text(&selected)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for font in context.available_fonts {
                                ui.selectable_value(&mut selected, font.clone(), font.clone());
                            }
                        });
                register_property_control(
                    context,
                    prop_def,
                    "font",
                    &response.response,
                    serde_json::json!(current_val),
                );

                if selected != current_val {
                    actions.push(PropertyAction::Update(
                        prop_def.name().to_string(),
                        PropertyValue::String(selected),
                    ));
                    actions.push(PropertyAction::Commit);
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Text | PropertyUiType::MultilineText => {
                let val_opt = authored_value.clone();
                if val_opt.is_none() {
                    log::warn!(
                        "[WARN] Missing value for Text property '{}'",
                        prop_def.name()
                    );
                }
                let current_val = val_opt
                    .and_then(|v| v.get_as::<String>())
                    .unwrap_or(prop_def.default_value().get_as().unwrap_or_default());
                let mut text = current_val.clone();
                let response = if matches!(prop_def.ui_type(), PropertyUiType::MultilineText) {
                    ui.text_edit_multiline(&mut text)
                } else {
                    ui.text_edit_singleline(&mut text)
                };
                register_property_control(
                    context,
                    prop_def,
                    "text",
                    &response,
                    serde_json::json!(current_val),
                );

                let new_val = if response.changed() {
                    Some(PropertyValue::String(text))
                } else {
                    None
                };

                handle_prop_response(
                    &mut actions,
                    &response,
                    prop_def.name(),
                    new_val,
                    prop_def.default_value(),
                );

                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Vec2 { .. }
            | PropertyUiType::Vec3 { .. }
            | PropertyUiType::Vec4 { .. } => {
                actions.extend(vector::render(ui, prop_def, authored_value, context));
                if context.in_grid {
                    ui.end_row();
                }
            }
        }
    }
    actions
}

impl Clone for PropertyRenderContext<'_> {
    fn clone(&self) -> Self {
        Self {
            available_fonts: self.available_fonts,
            in_grid: self.in_grid,
            current_time: self.current_time,
            qa_scope: self.qa_scope.clone(),
        }
    }
}

fn register_property_control(
    context: &PropertyRenderContext,
    definition: &PropertyDefinition,
    control_kind: &str,
    response: &egui::Response,
    value: serde_json::Value,
) {
    let property_name = definition.name();
    let component_id = format!("inspector.property.{}:{}", context.qa_scope, property_name);
    #[cfg(test)]
    INSPECTOR_TEST_RECTS.with(|rects| {
        rects
            .borrow_mut()
            .insert(component_id.clone(), response.rect);
    });
    crate::qa::register_component_with_metadata(
        component_id,
        "inspector_property_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "scope": context.qa_scope,
            "property": property_name,
            "control_kind": control_kind,
            "current_time": context.current_time,
            "value": value,
            "definition": property_definition_metadata(definition),
        })),
    );
}

#[cfg(test)]
thread_local! {
    static INSPECTOR_TEST_RECTS: std::cell::RefCell<std::collections::HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn property_definition_metadata(definition: &PropertyDefinition) -> serde_json::Value {
    let ui = match definition.ui_type() {
        PropertyUiType::Float {
            min,
            max,
            step,
            suffix,
            min_hard_limit,
            max_hard_limit,
        } => serde_json::json!({
            "kind": "float",
            "min": min,
            "max": max,
            "step": step,
            "suffix": suffix,
            "min_hard_limit": min_hard_limit,
            "max_hard_limit": max_hard_limit,
        }),
        PropertyUiType::Integer {
            min,
            max,
            suffix,
            min_hard_limit,
            max_hard_limit,
        } => serde_json::json!({
            "kind": "integer",
            "min": min,
            "max": max,
            "suffix": suffix,
            "min_hard_limit": min_hard_limit,
            "max_hard_limit": max_hard_limit,
        }),
        PropertyUiType::Dropdown { options } => {
            serde_json::json!({"kind": "dropdown", "options": options})
        }
        PropertyUiType::Vec2 {
            min,
            max,
            step,
            suffix,
            min_hard_limit,
            max_hard_limit,
        } => serde_json::json!({
            "kind": "vec2",
            "min": min,
            "max": max,
            "step": step,
            "suffix": suffix,
            "min_hard_limit": min_hard_limit,
            "max_hard_limit": max_hard_limit,
        }),
        PropertyUiType::Vec3 {
            min,
            max,
            step,
            suffix,
            min_hard_limit,
            max_hard_limit,
        } => serde_json::json!({
            "kind": "vec3",
            "min": min,
            "max": max,
            "step": step,
            "suffix": suffix,
            "min_hard_limit": min_hard_limit,
            "max_hard_limit": max_hard_limit,
        }),
        PropertyUiType::Vec4 {
            min,
            max,
            step,
            suffix,
            min_hard_limit,
            max_hard_limit,
        } => serde_json::json!({
            "kind": "vec4",
            "min": min,
            "max": max,
            "step": step,
            "suffix": suffix,
            "min_hard_limit": min_hard_limit,
            "max_hard_limit": max_hard_limit,
        }),
        PropertyUiType::Color => serde_json::json!({"kind": "color"}),
        PropertyUiType::Text => serde_json::json!({"kind": "text"}),
        PropertyUiType::MultilineText => serde_json::json!({"kind": "multiline_text"}),
        PropertyUiType::Bool => serde_json::json!({"kind": "boolean"}),
        PropertyUiType::Font => serde_json::json!({"kind": "font"}),
    };
    serde_json::json!({
        "name": definition.name(),
        "label": definition.label(),
        "default": serde_json::Value::from(definition.default_value()),
        "ui": ui,
    })
}

// Helper to standardise Grid + Property Evaluation loop
#[allow(
    dead_code,
    clippy::too_many_arguments,
    reason = "retained only for the legacy embedded extension renderers while Timeline Inspector uses operation Nodes"
)]
pub fn render_inspector_properties_grid(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    properties: &library::model::property::PropertyMap,
    definitions: &[PropertyDefinition],
    project_service: &library::EditorService,
    context: &PropertyRenderContext,
    fps: f64,
    resolution: (u64, u64),
) -> Vec<PropertyAction> {
    let mut pending_actions = Vec::new();
    let evaluated = evaluate_property_map(
        project_service,
        properties,
        context.current_time,
        fps,
        resolution,
    );
    render_evaluation_issues(ui, &context.qa_scope, evaluated.issues());

    egui::Grid::new(id).striped(true).show(ui, |ui| {
        // Force in_grid to true for this component
        let grid_context = PropertyRenderContext {
            in_grid: true,
            ..context.clone()
        };

        let actions = render_property_rows(
            ui,
            definitions,
            |name| evaluated.value(name).cloned(),
            |name| properties.get(name).cloned(),
            &grid_context,
        );
        pending_actions = actions;
    });

    pending_actions
}

#[allow(
    dead_code,
    reason = "retained only for the legacy embedded extension renderers while Timeline Inspector uses operation Nodes"
)]
pub fn render_add_button(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.menu_button("➕ Add", content);
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::plugin::{PluginManager, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY};

    #[test]
    fn qa_metadata_preserves_the_complete_property_definition() {
        let definition = PropertyDefinition::new(
            "width",
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: "px".to_string(),
                min_hard_limit: false,
                max_hard_limit: false,
            },
            "Width",
            PropertyValue::from(1.0),
        );
        assert_eq!(
            property_definition_metadata(&definition),
            serde_json::json!({
                "name": "width",
                "label": "Width",
                "default": 1.0,
                "ui": {
                    "kind": "float",
                    "min": 0.0,
                    "max": 100.0,
                    "step": 1.0,
                    "suffix": "px",
                    "min_hard_limit": false,
                    "max_hard_limit": false,
                },
            })
        );

        let join = PropertyDefinition::new(
            "join",
            PropertyUiType::Dropdown {
                options: vec![
                    "Miter".to_string(),
                    "Round".to_string(),
                    "Bevel".to_string(),
                ],
            },
            "Join",
            PropertyValue::String("Round".to_string()),
        );
        assert_eq!(
            property_definition_metadata(&join),
            serde_json::json!({
                "name": "join",
                "label": "Join",
                "default": "Round",
                "ui": {
                    "kind": "dropdown",
                    "options": ["Miter", "Round", "Bevel"],
                },
            })
        );
    }

    #[test]
    fn inspector_effector_float_control_responds_to_real_pointer_drag() {
        let plugins = PluginManager::default();
        let node = plugins.create_effector_operation_node("opacity").unwrap();
        let definition = plugins
            .operation_descriptor(EFFECTOR_CATEGORY, "opacity", EFFECTOR_APPLY_OPERATION)
            .unwrap()
            .properties()
            .iter()
            .find(|definition| definition.name() == "opacity")
            .unwrap()
            .clone();
        let property = node.properties().get("opacity").unwrap().clone();
        let render_context = PropertyRenderContext {
            available_fonts: &[],
            in_grid: false,
            current_time: 0.0,
            qa_scope: format!("node:{}", node.id),
        };
        let component_id = format!("inspector.property.node:{}:opacity", node.id);
        INSPECTOR_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0));
        let mut actions = Vec::new();

        for frame in 0..2 {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        actions.extend(render_property_rows(
                            ui,
                            std::slice::from_ref(&definition),
                            |_| property.evaluate_at(0.0).ok(),
                            |_| Some(property.clone()),
                            &render_context,
                        ));
                    });
                },
            );
            drop(output);
        }
        let rect = INSPECTOR_TEST_RECTS.with(|rects| {
            rects
                .borrow()
                .get(&component_id)
                .copied()
                .expect("rendered Inspector Effector property control")
        });
        let start = rect.center();
        let end = start + egui::vec2(48.0, 0.0);
        let event_frames = [
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
                    time: Some((offset + 2) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        actions.extend(render_property_rows(
                            ui,
                            std::slice::from_ref(&definition),
                            |_| property.evaluate_at(0.0).ok(),
                            |_| Some(property.clone()),
                            &render_context,
                        ));
                    });
                },
            );
            drop(output);
        }

        assert!(actions.iter().any(|action| matches!(
            action,
            PropertyAction::Update(name, PropertyValue::Number(value))
                if name == "opacity" && value.into_inner() > 0.0
        )));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, PropertyAction::Commit)),
            "pointer release must commit one Inspector drag gesture"
        );
    }
}
