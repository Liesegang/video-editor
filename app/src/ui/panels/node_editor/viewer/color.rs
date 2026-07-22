use super::ProjectNodeViewer;
use crate::ui::panels::node_editor::evaluate_node_property;
use crate::ui::panels::node_editor::{
    bounded_non_selectable_label, clipped_qa_rect, node_property_time, property_label,
    qa_rect_metadata, NodeEdit, INLINE_CONTROL_WIDTH,
};
use crate::ui::widgets::color_value_picker::color_value_picker;
use eframe::egui;
use library::model::project::PortOwner;
use library::model::property::{ColorSpaceRef, ColorValue, PropertyValue};
use library::model::ColorContent;
use ordered_float::OrderedFloat;
use uuid::Uuid;

impl ProjectNodeViewer<'_> {
    pub(super) fn show_color_body(
        &mut self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        operation: ColorContent,
    ) {
        ui.horizontal(|ui| {
            property_label(ui, "Category");
            bounded_non_selectable_label(ui, "Color", INLINE_CONTROL_WIDTH, egui::Align::LEFT);
        });
        ui.horizontal(|ui| {
            property_label(ui, "Operation");
            bounded_non_selectable_label(
                ui,
                operation.label(),
                INLINE_CONTROL_WIDTH,
                egui::Align::LEFT,
            );
        });
        if operation == ColorContent::Compose {
            self.show_compose_picker(ui, node_id);
        }
    }

    fn show_compose_picker(&mut self, ui: &mut egui::Ui, node_id: Uuid) {
        let Some(node) = self.project.get_node(node_id) else {
            return;
        };
        let linked_inputs = crate::utils::property::linked_node_inputs(
            self.project,
            node_id,
            &[
                library::model::COLOR_SPACE_PORT,
                library::model::COLOR_RED_PORT,
                library::model::COLOR_GREEN_PORT,
                library::model::COLOR_BLUE_PORT,
                library::model::COLOR_ALPHA_PORT,
            ],
        );
        let read_only = !linked_inputs.is_empty();
        let linked_ports = linked_inputs
            .iter()
            .map(|(port, _)| port.clone())
            .collect::<Vec<_>>();
        let time = node_property_time(
            self.project,
            self.plugin_manager,
            node_id,
            self.current_time,
        );
        let Some(authored_color) = compose_color(|key| {
            node.properties().get(key).and_then(|property| {
                evaluate_node_property(self.project, self.plugin_manager, node_id, property, time)
                    .value()
                    .cloned()
            })
        }) else {
            ui.colored_label(ui.visuals().error_fg_color, "Color channels are incomplete");
            return;
        };
        let resolved_color = read_only.then(|| {
            self.plugin_manager.and_then(|plugin_manager| {
                match crate::utils::property::evaluate_node_metadata_output(
                    self.project,
                    plugin_manager,
                    node_id,
                    library::model::COLOR_VALUE_PORT,
                    self.current_time,
                ) {
                    Ok(library::model::project::EvalOutput::Produced(
                        PropertyValue::ColorValue(color),
                    )) => Some(color),
                    Ok(_) => None,
                    Err(error) => {
                        log::warn!("Cannot resolve linked Compose color {node_id}: {error}");
                        None
                    }
                }
            })
        });
        let resolved_color = resolved_color.flatten();
        let color = resolved_color.as_ref().unwrap_or(&authored_color);
        ui.horizontal(|ui| {
            property_label(
                ui,
                if resolved_color.is_some() {
                    "Result"
                } else if read_only {
                    "Fallback"
                } else {
                    "Picker"
                },
            );
            let mut picker = ui
                .add_enabled_ui(!read_only, |ui| {
                    color_value_picker(
                        ui,
                        egui::Id::new(("node_editor_compose_color_picker", node_id)),
                        color,
                    )
                })
                .inner;
            let unclipped_rect = *self.to_global * picker.response.rect;
            let rect = clipped_qa_rect(unclipped_rect, *self.canvas_clip);
            crate::qa::register_component_with_metadata(
                format!("node_editor.color_picker.node:{node_id}:compose"),
                "node_editor_compose_color_picker",
                rect,
                picker.response.enabled(),
                Some(serde_json::json!({
                    "node_id": node_id,
                    "operation": "native.color.compose",
                    "authored_space": authored_color.color_space().as_str(),
                    "displayed_space": color.color_space().as_str(),
                    "display_space": "srgb",
                    "transform_authority": "ruvie-color-management",
                    "grouped_properties": ["space", "r", "g", "b", "a"],
                    "linked_inputs": linked_inputs.iter().map(|(port, source)| serde_json::json!({
                        "port": port,
                        "source": source,
                    })).collect::<Vec<_>>(),
                    "editable": !read_only,
                    "displayed_value": if resolved_color.is_some() { "resolved_runtime_output" } else if read_only { "authored_fallback_unavailable_runtime" } else { "evaluated_properties" },
                    "resolved_value": resolved_color.as_ref().map(|color| serde_json::Value::from(&PropertyValue::ColorValue(color.clone()))),
                    "unclipped_rect": qa_rect_metadata(unclipped_rect),
                })),
            );
            let edit = (!read_only)
                .then(|| picker.value.take())
                .flatten()
                .map(|color| NodeEdit::SetNodeProperties {
                    node_id,
                    time,
                    values: color_component_values(&color),
                });
            self.queue_continuous_edit(
                PortOwner::Node(node_id),
                "$compose_color_picker",
                edit,
                !read_only && picker.finished,
            );
            if read_only {
                bounded_non_selectable_label(
                    ui,
                    format!("linked result: {}", linked_ports.join(", ")),
                    INLINE_CONTROL_WIDTH,
                    egui::Align::LEFT,
                )
                .on_hover_text(if resolved_color.is_some() {
                    "Read-only runtime result from the connected ports; edit their source Nodes."
                } else {
                    "The connected runtime result is unavailable at this time; this swatch is only the authored fallback."
                });
            }
        });
    }
}

fn compose_color(mut value: impl FnMut(&str) -> Option<PropertyValue>) -> Option<ColorValue> {
    let space = value(library::model::COLOR_SPACE_PORT)?.get_as::<String>()?;
    let mut component = |key: &str| value(key)?.get_as::<f64>();
    ColorValue::new(
        ColorSpaceRef::new(space).ok()?,
        [
            component(library::model::COLOR_RED_PORT)?,
            component(library::model::COLOR_GREEN_PORT)?,
            component(library::model::COLOR_BLUE_PORT)?,
            component(library::model::COLOR_ALPHA_PORT)?,
        ],
    )
    .ok()
}

fn color_component_values(color: &ColorValue) -> Vec<(String, PropertyValue)> {
    let mut values = vec![(
        library::model::COLOR_SPACE_PORT.to_string(),
        PropertyValue::String(color.color_space().to_string()),
    )];
    values.extend(
        [
            library::model::COLOR_RED_PORT,
            library::model::COLOR_GREEN_PORT,
            library::model::COLOR_BLUE_PORT,
            library::model::COLOR_ALPHA_PORT,
        ]
        .into_iter()
        .zip(color.rgba())
        .map(|(key, value)| (key.to_string(), PropertyValue::Number(OrderedFloat(value))))
        .collect::<Vec<_>>(),
    );
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::node_editor::apply_edit;
    use library::animation::EasingFunction;
    use library::model::property::{Keyframe, Property};
    use library::model::Project;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    #[test]
    fn grouped_compose_picker_edit_is_atomic_and_preserves_channel_keyframes() {
        let mut node = library::model::Node::new_color("Compose", ColorContent::Compose);
        let node_id = node.id;
        node.set_property(
            library::model::COLOR_RED_PORT.to_string(),
            Property::keyframe(vec![Keyframe::new(
                2.0,
                number(0.1),
                EasingFunction::Linear,
            )]),
        )
        .unwrap();
        let mut project = Project::new("compose picker");
        project.add_node(node);

        assert!(apply_edit(
            &mut project,
            NodeEdit::SetNodeProperties {
                node_id,
                time: 2.0,
                values: vec![
                    (
                        library::model::COLOR_SPACE_PORT.to_string(),
                        PropertyValue::String(ColorSpaceRef::linear_srgb().to_string()),
                    ),
                    (library::model::COLOR_RED_PORT.to_string(), number(0.25)),
                    (library::model::COLOR_GREEN_PORT.to_string(), number(0.5)),
                    (library::model::COLOR_BLUE_PORT.to_string(), number(0.75)),
                    (library::model::COLOR_ALPHA_PORT.to_string(), number(0.625)),
                ],
            },
        ));
        let node = project.get_node(node_id).unwrap();
        let red = node
            .properties()
            .get(library::model::COLOR_RED_PORT)
            .unwrap();
        assert_eq!(red.evaluator, "keyframe");
        assert_eq!(
            red.keyframes()
                .into_iter()
                .find(|keyframe| keyframe.time == OrderedFloat(2.0))
                .map(|keyframe| keyframe.value),
            Some(number(0.25))
        );
        assert_eq!(
            node.properties()
                .get(library::model::COLOR_SPACE_PORT)
                .and_then(Property::value),
            Some(&PropertyValue::String("linear-srgb".to_string()))
        );

        let before_invalid = project.clone();
        assert!(!apply_edit(
            &mut project,
            NodeEdit::SetNodeProperties {
                node_id,
                time: 2.0,
                values: vec![
                    (library::model::COLOR_RED_PORT.to_string(), number(0.9)),
                    (library::model::COLOR_ALPHA_PORT.to_string(), number(2.0)),
                ],
            },
        ));
        assert_eq!(project, before_invalid);
    }
}
