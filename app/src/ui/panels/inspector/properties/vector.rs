use egui::Ui;
use library::model::property::{
    PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4,
};
use ordered_float::OrderedFloat;

use super::{register_property_control, PropertyAction, PropertyRenderContext};
use crate::ui::widgets::property_drag_value::FloatDragValueConfig;
use crate::ui::widgets::vector_drag_value::{vector_drag_values, VectorAxisResponse};

const AXES: [&str; 4] = ["X", "Y", "Z", "W"];

pub(super) fn render(
    ui: &mut Ui,
    definition: &PropertyDefinition,
    value: Option<PropertyValue>,
    context: &PropertyRenderContext<'_>,
) -> Vec<PropertyAction> {
    let Some((control_kind, current, mut components)) = vector_components(definition, value) else {
        log::error!(
            "Vector property '{}' has invalid numeric metadata",
            definition.name()
        );
        ui.colored_label(ui.visuals().error_fg_color, "Invalid vector metadata");
        return Vec::new();
    };
    let Some(config) = FloatDragValueConfig::from_definition(definition) else {
        log::error!(
            "Vector property '{}' has invalid drag metadata",
            definition.name()
        );
        ui.colored_label(ui.visuals().error_fg_color, "Invalid vector metadata");
        return Vec::new();
    };
    let mut axis_values = AXES
        .iter()
        .copied()
        .zip(components.iter_mut())
        .collect::<Vec<_>>();
    let control_height = ui.spacing().interact_size.y;
    let group = vector_drag_values(ui, &config, &mut axis_values, control_height);
    register_property_control(
        context,
        definition,
        control_kind,
        &group.response,
        serde_json::Value::from(&current),
    );
    register_components(context, definition, &group.axes);

    if group.reset {
        return vec![
            PropertyAction::Update(
                definition.name().to_string(),
                definition.default_value().clone(),
            ),
            PropertyAction::Commit,
        ];
    }

    let mut actions = Vec::new();
    if group.changed {
        let Some(value) = property_value(definition.ui_type(), &components) else {
            log::error!(
                "Vector property '{}' changed with an incompatible component count",
                definition.name()
            );
            return actions;
        };
        actions.push(PropertyAction::Update(definition.name().to_string(), value));
    }
    if group.finished {
        actions.push(PropertyAction::Commit);
    }
    actions
}

fn vector_components(
    definition: &PropertyDefinition,
    value: Option<PropertyValue>,
) -> Option<(&'static str, PropertyValue, Vec<f64>)> {
    match definition.ui_type() {
        PropertyUiType::Vec2 { .. } => {
            let value = value
                .and_then(|value| value.get_as::<Vec2>())
                .or_else(|| definition.default_value().get_as::<Vec2>())
                .unwrap_or(Vec2 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                });
            Some((
                "vec2",
                PropertyValue::Vec2(value),
                vec![value.x.into_inner(), value.y.into_inner()],
            ))
        }
        PropertyUiType::Vec3 { .. } => {
            let value = value
                .and_then(|value| value.get_as::<Vec3>())
                .or_else(|| definition.default_value().get_as::<Vec3>())
                .unwrap_or(Vec3 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                    z: OrderedFloat(0.0),
                });
            Some((
                "vec3",
                PropertyValue::Vec3(value),
                vec![
                    value.x.into_inner(),
                    value.y.into_inner(),
                    value.z.into_inner(),
                ],
            ))
        }
        PropertyUiType::Vec4 { .. } => {
            let value = value
                .and_then(|value| value.get_as::<Vec4>())
                .or_else(|| definition.default_value().get_as::<Vec4>())
                .unwrap_or(Vec4 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                    z: OrderedFloat(0.0),
                    w: OrderedFloat(0.0),
                });
            Some((
                "vec4",
                PropertyValue::Vec4(value),
                vec![
                    value.x.into_inner(),
                    value.y.into_inner(),
                    value.z.into_inner(),
                    value.w.into_inner(),
                ],
            ))
        }
        _ => None,
    }
}

fn property_value(ui_type: &PropertyUiType, components: &[f64]) -> Option<PropertyValue> {
    match (ui_type, components) {
        (PropertyUiType::Vec2 { .. }, [x, y]) => Some(PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(*x),
            y: OrderedFloat(*y),
        })),
        (PropertyUiType::Vec3 { .. }, [x, y, z]) => Some(PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(*x),
            y: OrderedFloat(*y),
            z: OrderedFloat(*z),
        })),
        (PropertyUiType::Vec4 { .. }, [x, y, z, w]) => Some(PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(*x),
            y: OrderedFloat(*y),
            z: OrderedFloat(*z),
            w: OrderedFloat(*w),
        })),
        _ => None,
    }
}

fn register_components(
    context: &PropertyRenderContext<'_>,
    definition: &PropertyDefinition,
    components: &[VectorAxisResponse],
) {
    for component in components {
        let axis = component.axis.to_ascii_lowercase();
        let component_id = format!(
            "inspector.property_component.{}:{}:{axis}",
            context.qa_scope,
            definition.name(),
        );
        #[cfg(test)]
        super::INSPECTOR_TEST_RECTS.with(|rects| {
            rects
                .borrow_mut()
                .insert(component_id.clone(), component.response.rect);
        });
        crate::qa::register_component_with_metadata(
            component_id,
            "inspector_vector_component_control",
            component.response.rect,
            component.response.enabled(),
            Some(serde_json::json!({
                "scope": context.qa_scope,
                "property": definition.name(),
                "axis": axis,
                "control_kind": "float",
                "current_time": context.current_time,
                "value": component.value,
                "definition": super::property_definition_metadata(definition),
            })),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::plugin::{
        PluginManager, SHAPE_TRANSFORM_COMPONENT_ID, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
    };

    #[test]
    fn inspector_vector_component_responds_to_real_pointer_drag() {
        let plugins = PluginManager::default();
        let node = plugins
            .create_shape_transform_operation_node()
            .expect("built-in Shape Transform node");
        let definition = plugins
            .operation_descriptor(
                TRANSFORM_CATEGORY,
                SHAPE_TRANSFORM_COMPONENT_ID,
                TRANSFORM_APPLY_OPERATION,
            )
            .expect("built-in Shape Transform descriptor")
            .properties()
            .iter()
            .find(|definition| definition.name() == "position")
            .expect("Shape Transform position definition")
            .clone();
        let value = node
            .properties()
            .get("position")
            .expect("Shape Transform position property")
            .evaluate_at(0.0)
            .expect("position evaluates");
        let render_context = PropertyRenderContext {
            available_fonts: &[],
            in_grid: false,
            current_time: 0.0,
            qa_scope: format!("node:{}", node.id),
        };
        let x_component_id = format!("inspector.property_component.node:{}:position:x", node.id);
        super::super::INSPECTOR_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 160.0));
        let mut actions = Vec::new();

        for frame in 0..2 {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        actions.extend(render(
                            ui,
                            &definition,
                            Some(value.clone()),
                            &render_context,
                        ));
                    });
                },
            ));
        }
        let rect = super::super::INSPECTOR_TEST_RECTS.with(|rects| {
            rects
                .borrow()
                .get(&x_component_id)
                .copied()
                .expect("Inspector registered the exact position X component")
        });
        let start = rect.center();
        let end = start + egui::vec2(48.0, 0.0);
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
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((offset + 2) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        actions.extend(render(
                            ui,
                            &definition,
                            Some(value.clone()),
                            &render_context,
                        ));
                    });
                },
            ));
        }

        assert!(actions.iter().any(|action| matches!(
            action,
            PropertyAction::Update(name, PropertyValue::Vec2(position))
                if name == "position"
                    && position.x.into_inner() > 0.0
                    && position.y == OrderedFloat(0.0)
        )));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, PropertyAction::Commit)),
            "pointer release must commit one Inspector vector gesture"
        );
    }

    #[test]
    fn vector_value_conversion_rejects_mismatched_arity() {
        assert!(property_value(&PropertyUiType::vec2("px"), &[1.0]).is_none());
        assert!(property_value(&PropertyUiType::vec3("px"), &[1.0, 2.0]).is_none());
        assert!(property_value(&PropertyUiType::vec4("px"), &[1.0, 2.0, 3.0]).is_none());
    }
}
