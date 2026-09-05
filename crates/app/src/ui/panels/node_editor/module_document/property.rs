//! Module property editing through the same typed authoring controls used by
//! the Inspector and by the original production Node Editor.

use egui_phosphor::regular as icons;
use library::plugin::EvaluationContext;

use super::*;
use crate::ui::panels::node_editor::property_label;
use crate::ui::widgets::property_mode::{
    property_for_mode, property_mode_control_for_state, toggled_keyframe_property,
    PropertyModeAction, PropertyModeState,
};
use crate::ui::widgets::property_value_editor::{property_value_editor, PropertyValueEditorSpec};

#[allow(
    clippy::too_many_arguments,
    reason = "A Node Editor property row needs plugin evaluation, authored property metadata, connection state, and frame evaluation context at one immediate-mode UI boundary"
)]
pub(super) fn show_property_input(
    ui: &mut egui::Ui,
    plugins: &PluginManager,
    node: &Node,
    key: &str,
    property: &Property,
    definition: Option<&PropertyDefinition>,
    connected: bool,
    context: ModulePropertyContext,
    qa_transform: egui::emath::TSTransform,
    palette: &library::model::authoring::ProjectPalette,
) -> (egui::Response, Option<ModuleEditorAction>) {
    let evaluator_context =
        EvaluationContext::new(node.properties(), context.fps, context.resolution);
    let evaluated = plugins.get_property_evaluators().evaluate_with_diagnostics(
        property,
        context.time,
        &evaluator_context,
    );
    let (mut value, diagnostic) = match evaluated {
        Ok(outcome) => (
            Some(outcome.value().clone()),
            outcome
                .diagnostic()
                .map(|diagnostic| format!("{}: {}", diagnostic.evaluator(), diagnostic.message())),
        ),
        Err(error) => (property.value().cloned(), Some(error.to_string())),
    };
    let mode_value = value
        .clone()
        .or_else(|| property.value().cloned())
        .or_else(|| definition.map(|definition| definition.default_value().clone()));
    let label = definition.map_or(key, PropertyDefinition::label);
    let qa_id = format!("node_editor.property.node:{}:{key}", node.id);
    let mut action = None;
    let dynamic_value_disabled_reason = node_property_dynamic_value_disabled_reason(node, key);
    let allow_expression = dynamic_value_disabled_reason.is_none()
        && definition.map_or_else(
            || {
                mode_value
                    .as_ref()
                    .is_none_or(PropertyValue::supports_expression)
            },
            |definition| definition.ui_type().supports_expression(),
        );

    let row = ui.horizontal(|ui| {
        property_label(ui, label).on_hover_text(label);

        let mode_state = PropertyModeState::from_property(Some(property), context.time, false);
        let (mode_action, mode_response) = property_mode_control_for_state(
            ui,
            &format!("node_editor.property_mode.node:{}:{key}", node.id),
            mode_state,
            false,
            dynamic_value_disabled_reason,
            allow_expression,
        );
        if let (Some(mode_action), Some(current_value)) = (mode_action, mode_value.clone()) {
            let replacement = match mode_action {
                PropertyModeAction::SetMode(mode) => {
                    property_for_mode(Some(property), mode, current_value, context.time).ok()
                }
                PropertyModeAction::ToggleKeyframe => {
                    toggled_keyframe_property(property, current_value, context.time)
                }
            };
            if let Some(property) = replacement {
                action = Some(ModuleEditorAction::SetNodeProperty {
                    node_id: node.id,
                    key: key.to_string(),
                    property,
                });
            }
        }

        if let Some(diagnostic) = diagnostic.as_deref() {
            ui.colored_label(ui.visuals().warn_fg_color, icons::WARNING)
                .on_hover_text(diagnostic);
        }

        if connected {
            ui.weak("Connected").on_hover_text(
                "The connected input controls the effective value; disconnect it to edit here",
            );
            return mode_response;
        }
        let Some(value) = value.as_mut() else {
            return ui.weak("No value");
        };
        let edit = property_value_editor(
            ui,
            egui::Id::new(("module_property", node.id, key)),
            &qa_id,
            value,
            PropertyValueEditorSpec {
                definition,
                fallback_suffix: "",
                fallback_speed: 0.05,
                palette,
            },
        );
        if edit.changed {
            action = Some(ModuleEditorAction::SetNodeProperty {
                node_id: node.id,
                key: key.to_string(),
                property: property_with_edited_value(property, value.clone(), context.time),
            });
        }
        edit.response
    });

    let response = row.inner;
    crate::qa::register_component_with_metadata(
        &qa_id,
        "node_property_control",
        qa_transform * response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "document_kind": "module_definition",
            "node_id": node.id,
            "property": key,
            "connected": connected,
            "evaluator": property.evaluator,
            "descriptor_available": definition.is_some(),
            "allow_expression": allow_expression,
            "dynamic_value_disabled_reason": dynamic_value_disabled_reason,
            "current_time": context.time,
            "evaluation_diagnostic": diagnostic,
        })),
    );
    (response, action)
}

fn node_property_dynamic_value_disabled_reason(
    node: &Node,
    property_key: &str,
) -> Option<&'static str> {
    library::model::native_node_descriptor_for_node(node)?
        .dynamic_input_disabled_reason(property_key)
}

pub(super) fn property_with_edited_value(
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
            // Expression and plugin-defined evaluators retain their authored
            // source/configuration; only their typed fallback/value changes.
            replacement.properties.insert("value".to_string(), value);
            replacement
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::animation::EasingFunction;
    use library::model::property::Vec2;
    use ordered_float::OrderedFloat;

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
    }

    #[test]
    fn vector_property_edit_keeps_the_other_axis_and_expression_source() {
        let original = Property::expression(
            "value + time".to_string(),
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(10.0),
                y: OrderedFloat(20.0),
            }),
        );
        let edited = property_with_edited_value(
            &original,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(30.0),
                y: OrderedFloat(20.0),
            }),
            4.0,
        );
        assert_eq!(edited.expression_text(), Some("value + time"));
        assert_eq!(
            edited.value(),
            Some(&PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(30.0),
                y: OrderedFloat(20.0),
            }))
        );
    }

    #[test]
    fn native_particle_dynamic_input_uses_the_catalog_capability() {
        let emitter = Node::new_catalog_node("native.particle.emitter").expect("Particle Emitter");
        assert!(
            node_property_dynamic_value_disabled_reason(&emitter, "rate")
                .is_some_and(|reason| reason.contains("fixed-step"))
        );
    }
}
