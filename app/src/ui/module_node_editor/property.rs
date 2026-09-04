//! Typed property controls rendered inside Module nodes.

use super::*;

pub(super) struct ModuleBodyRenderer<'a> {
    pub(super) nodes: &'a HashMap<Uuid, Node>,
    pub(super) connected_inputs: &'a HashSet<ModulePortAddress>,
    pub(super) plugin_manager: &'a PluginManager,
    pub(super) property_time: f64,
    pub(super) actions: &'a mut Vec<ModuleEditorAction>,
}

impl NodeBodyRenderer<Uuid> for ModuleBodyRenderer<'_> {
    fn show(&mut self, node_id: &Uuid, ui: &mut egui::Ui) -> NodeBodyResponse {
        let Some(node) = self.nodes.get(node_id) else {
            return NodeBodyResponse::NONE;
        };
        let mut ownership = NodeBodyResponse::NONE;
        ui.horizontal(|ui| {
            ui.add_space(BODY_INPUT_GUTTER);
            let mut enabled = node.enabled;
            let enabled_response = ui.checkbox(&mut enabled, "Enabled");
            ownership = ownership.union(NodeBodyResponse::from_response(&enabled_response));
            let mut bypassed = node.bypassed;
            let bypass_response = ui.add_enabled(
                node.supports_bypass(),
                egui::Checkbox::new(&mut bypassed, "Bypass"),
            );
            ownership = ownership.union(NodeBodyResponse::from_response(&bypass_response));
            if enabled_response.changed() || bypass_response.changed() {
                self.actions.push(ModuleEditorAction::SetNodeState {
                    node_id: *node_id,
                    name: node.name.clone(),
                    enabled,
                    bypassed,
                });
            }
        });

        let mut properties = node.properties().iter().collect::<Vec<_>>();
        properties.sort_by(|left, right| left.0.cmp(right.0));
        for (key, property) in properties {
            let definition = node_property_definition(self.plugin_manager, node, key);
            let connected = self.connected_inputs.iter().any(|address| {
                address.node_id == *node_id
                    && authored_property_key_for_port(node.properties(), &address.port)
                        == Some(key.as_str())
            });
            let (response, edited_value) =
                show_property_control(ui, *node_id, key, property, definition.as_ref(), connected);
            ownership = ownership.union(NodeBodyResponse::from_response(&response));
            if let Some(value) = edited_value {
                self.actions.push(ModuleEditorAction::SetNodeProperty {
                    node_id: *node_id,
                    key: key.clone(),
                    property: property_with_edited_value(property, value, self.property_time),
                });
            }
        }
        ownership
    }
}

fn property_with_edited_value(
    property: &Property,
    value: PropertyValue,
    property_time: f64,
) -> Property {
    let mut replacement = property.clone();
    match replacement.evaluator.as_str() {
        "constant" => Property::constant(value),
        "keyframe" => {
            let _ = replacement.upsert_keyframe(property_time, value, None);
            replacement
        }
        _ => {
            replacement.properties.insert("value".to_string(), value);
            replacement
        }
    }
}

fn node_property_definition(
    plugins: &PluginManager,
    node: &Node,
    property_name: &str,
) -> Option<PropertyDefinition> {
    let definitions: &[PropertyDefinition] = match node.content() {
        NodeContent::Value(value) => value.property_definitions(),
        NodeContent::Color(value) => value.property_definitions(),
        NodeContent::SoundAnalysis(value) => value.property_definitions(),
        NodeContent::Data(value) => value.property_definitions(),
        NodeContent::List(value) => value.property_definitions(),
        NodeContent::Path(value) => value.property_definitions(),
        NodeContent::PluginOperation(operation) => {
            return plugins
                .operation_descriptor(
                    &operation.category,
                    &operation.component_id,
                    &operation.operation,
                )
                .ok()?
                .properties()
                .iter()
                .find(|definition| definition.name() == property_name)
                .cloned();
        }
        NodeContent::Media(_)
        | NodeContent::Generator(_)
        | NodeContent::CompositionInstance(_)
        | NodeContent::NativeOperation(_)
        | NodeContent::Merge
        | NodeContent::SoundMerge => return None,
    };
    definitions
        .iter()
        .find(|definition| definition.name() == property_name)
        .cloned()
}

fn show_property_control(
    ui: &mut egui::Ui,
    node_id: Uuid,
    key: &str,
    property: &Property,
    definition: Option<&PropertyDefinition>,
    connected: bool,
) -> (egui::Response, Option<PropertyValue>) {
    let label = definition.map_or(key, PropertyDefinition::label);
    let mut edited = property.value().cloned();
    let row = ui.horizontal(|ui| {
        ui.add_space(BODY_INPUT_GUTTER);
        ui.add_sized(
            [70.0, PORT_ROW_HEIGHT],
            egui::Label::new(label).truncate().selectable(false),
        );
        if connected {
            return (ui.weak("Connected"), None);
        }
        let Some(value) = edited.as_mut() else {
            return (ui.weak("No value"), None);
        };
        let response = match value {
            PropertyValue::Number(number) => {
                if let Some(config) = definition.and_then(FloatDragValueConfig::from_definition) {
                    ui.add_sized([96.0, PORT_ROW_HEIGHT], config.widget(&mut number.0))
                } else {
                    ui.add_sized(
                        [96.0, PORT_ROW_HEIGHT],
                        egui::DragValue::new(&mut number.0).speed(0.05),
                    )
                }
            }
            PropertyValue::Integer(integer) => {
                if let Some(config) = definition.and_then(|definition| {
                    IntegerDragValueConfig::from_ui_type(definition.ui_type())
                }) {
                    ui.add_sized([96.0, PORT_ROW_HEIGHT], config.widget(integer))
                } else {
                    ui.add_sized([96.0, PORT_ROW_HEIGHT], egui::DragValue::new(integer))
                }
            }
            PropertyValue::String(text) => {
                if let Some(PropertyUiType::Dropdown { options }) =
                    definition.map(PropertyDefinition::ui_type)
                {
                    egui::ComboBox::from_id_salt(("module_property", node_id, key))
                        .selected_text(text.as_str())
                        .width(104.0)
                        .show_ui(ui, |ui| {
                            for option in options {
                                ui.selectable_value(text, option.clone(), option);
                            }
                        })
                        .response
                } else {
                    ui.add_sized(
                        [116.0, PORT_ROW_HEIGHT],
                        egui::TextEdit::singleline(text).clip_text(true),
                    )
                }
            }
            PropertyValue::Boolean(boolean) => ui.checkbox(boolean, ""),
            PropertyValue::Color(color) => {
                let mut display =
                    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
                let response = ui.color_edit_button_srgba(&mut display);
                if response.changed() {
                    color.r = display.r();
                    color.g = display.g();
                    color.b = display.b();
                    color.a = display.a();
                }
                response
            }
            PropertyValue::ColorValue(color) => {
                let picker =
                    color_value_picker(ui, egui::Id::new(("module_color", node_id, key)), color);
                if let Some(value) = picker.value {
                    *color = value;
                }
                picker.response
            }
            PropertyValue::Vec2(value) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut value.x.0)
                            .prefix("x ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.y.0)
                            .prefix("y ")
                            .speed(0.05),
                    )
                })
                .response
            }
            PropertyValue::Vec3(value) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut value.x.0)
                            .prefix("x ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.y.0)
                            .prefix("y ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.z.0)
                            .prefix("z ")
                            .speed(0.05),
                    )
                })
                .response
            }
            PropertyValue::Vec4(value) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut value.x.0)
                            .prefix("x ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.y.0)
                            .prefix("y ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.z.0)
                            .prefix("z ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.w.0)
                            .prefix("w ")
                            .speed(0.05),
                    )
                })
                .response
            }
            PropertyValue::Path(_) => ui.weak("Path"),
            PropertyValue::Array(values) => ui.weak(format!("{} items", values.len())),
            PropertyValue::Map(values) => ui.weak(format!("{} fields", values.len())),
            PropertyValue::OpaqueJson(_) => ui.weak("Unsupported value"),
        };
        let value = response.changed().then(|| value.clone());
        (response, value)
    });
    let (response, changed) = row.inner;
    crate::qa::register_component_with_metadata(
        format!("node_editor.property.node:{node_id}:{key}"),
        "node_property_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "document_kind": "module_definition",
            "node_id": node_id,
            "property": key,
            "connected": connected,
            "evaluator": property.evaluator,
            "descriptor_available": definition.is_some(),
        })),
    );
    (response, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::animation::EasingFunction;

    #[test]
    fn property_edits_preserve_the_authored_evaluator_mode() {
        let constant = Property::constant(PropertyValue::Integer(1));
        let edited = property_with_edited_value(&constant, PropertyValue::Integer(2), 3.0);
        assert_eq!(edited.evaluator, "constant");
        assert_eq!(edited.value(), Some(&PropertyValue::Integer(2)));

        let keyframed = Property::keyframe(vec![library::model::property::Keyframe::new(
            0.0,
            PropertyValue::Integer(1),
            EasingFunction::Linear,
        )]);
        let edited = property_with_edited_value(&keyframed, PropertyValue::Integer(7), 3.0);
        assert_eq!(edited.evaluator, "keyframe");
        assert_eq!(edited.keyframes().len(), 2);
        assert!(edited.keyframes().iter().any(|keyframe| {
            keyframe.time.into_inner() == 3.0 && keyframe.value == PropertyValue::Integer(7)
        }));

        let expression = Property::expression("x * 2".to_string(), PropertyValue::Integer(1));
        let edited = property_with_edited_value(&expression, PropertyValue::Integer(9), 3.0);
        assert_eq!(edited.evaluator, "expression");
        assert_eq!(edited.expression_text(), Some("x * 2"));
        assert_eq!(edited.value(), Some(&PropertyValue::Integer(9)));
    }
}
