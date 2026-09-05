use super::*;

pub(super) fn source_properties(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
) {
    let properties = match &item.source {
        SourceRef::Solid { color } => vec![SourceProperty::fallback(
            "color",
            "Color",
            PropertyValue::Color(color.clone()),
            "",
            0.01,
        )],
        SourceRef::Shape { shape } => {
            let mut properties = Vec::new();
            if shape.shape_kind != library::model::authoring::ShapeKind::Path {
                for (key, label) in [("width", "Width"), ("height", "Height")] {
                    properties.push(SourceProperty::fallback(
                        key,
                        label,
                        shape
                            .parameters
                            .get(key)
                            .cloned()
                            .unwrap_or_else(|| PropertyValue::from(100.0)),
                        " px",
                        1.0,
                    ));
                }
            }
            properties
        }
        SourceRef::Text { text, .. } => text_source_properties(text),
        _ => return,
    };
    egui::CollapsingHeader::new("Source")
        .default_open(true)
        .show(ui, |ui| {
            for property in &properties {
                let spec = ItemPropertySpec {
                    key: &property.key,
                    label: &property.label,
                    default: property.default.clone(),
                    definition: property.definition.as_ref(),
                    suffix: property.suffix,
                    speed: property.speed,
                };
                match property.target {
                    SourcePropertyTarget::Authored => {
                        show_item_property(ui, project, state, service, item, spec);
                    }
                    SourcePropertyTarget::TextContent => {
                        show_text_content_property(ui, project, state, service, item, spec);
                    }
                }
            }
        });
}

fn text_source_properties(text: &str) -> Vec<SourceProperty> {
    let definitions = library::plugin::entity_converter::timeline_text_property_definitions();
    let definition = |key: &str| {
        definitions
            .iter()
            .find(|candidate| candidate.name() == key)
            .cloned()
    };
    let Some(content) = definition("text") else {
        return Vec::new();
    };
    let Some(font) = definition("font_family") else {
        return Vec::new();
    };
    let Some(size) = definition("size") else {
        return Vec::new();
    };
    vec![
        SourceProperty::text_content(content, text),
        SourceProperty::from_definition(font, "", 0.1),
        SourceProperty::from_definition(size, " px", 1.0),
    ]
}

struct SourceProperty {
    key: String,
    label: String,
    default: PropertyValue,
    definition: Option<library::model::property::PropertyDefinition>,
    suffix: &'static str,
    speed: f64,
    target: SourcePropertyTarget,
}

#[derive(Clone, Copy)]
enum SourcePropertyTarget {
    Authored,
    TextContent,
}

impl SourceProperty {
    fn fallback(
        key: impl Into<String>,
        label: impl Into<String>,
        default: PropertyValue,
        suffix: &'static str,
        speed: f64,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            default,
            definition: None,
            suffix,
            speed,
            target: SourcePropertyTarget::Authored,
        }
    }

    fn from_definition(
        definition: library::model::property::PropertyDefinition,
        suffix: &'static str,
        speed: f64,
    ) -> Self {
        Self {
            key: definition.name().to_string(),
            label: definition.label().to_string(),
            default: definition.default_value().clone(),
            definition: Some(definition),
            suffix,
            speed,
            target: SourcePropertyTarget::Authored,
        }
    }

    fn text_content(definition: library::model::property::PropertyDefinition, text: &str) -> Self {
        Self {
            key: definition.name().to_string(),
            label: definition.label().to_string(),
            default: PropertyValue::String(text.to_string()),
            definition: Some(definition),
            suffix: "",
            speed: 0.1,
            target: SourcePropertyTarget::TextContent,
        }
    }
}

pub(super) struct ItemPropertySpec<'a> {
    pub(super) key: &'a str,
    pub(super) label: &'a str,
    pub(super) default: PropertyValue,
    pub(super) definition: Option<&'a library::model::property::PropertyDefinition>,
    pub(super) suffix: &'a str,
    pub(super) speed: f64,
}

/// One Timeline-owned property row shared by Transform and source controls.
pub(super) fn show_item_property(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    spec: ItemPropertySpec<'_>,
) {
    let ItemPropertySpec {
        key,
        label,
        default,
        definition,
        suffix,
        speed,
    } = spec;
    let draft_key = format!("authored:{key}");
    let local_time = item_local_time(project, state, item);
    let local_seconds = local_time
        .as_ref()
        .map_or(0.0, |time| time.to_seconds_f64());
    let authored = item.authored_properties.get(key);
    let mode_state = PropertyModeState::from_property(authored, local_seconds, true);
    let allow_expression = default.supports_expression();
    let initial = property_value_at(item, key, default, local_seconds);
    let model_value = initial.clone();
    let (finished, mode_action, edited_value) = ui
        .horizontal(|ui| {
            let (finished, mode_action, edited_value, publish_default) = {
                let value = state
                    .inspector
                    .property_values
                    .entry(draft_key)
                    .or_insert(initial);
                let result = property_row(
                    ui,
                    value,
                    &project.palette,
                    PropertyRowSpec {
                        control_id: &format!("item:{}:{key}", item.id),
                        label,
                        definition,
                        suffix,
                        speed,
                        mode_state,
                        allow_keyframe: true,
                        keyframe_disabled_reason: None,
                        allow_expression,
                    },
                );
                (
                    result.finished,
                    result.mode_action,
                    value.clone(),
                    value.clone(),
                )
            };
            composition_parameters::publication_icon(
                ui,
                project,
                state,
                service,
                item,
                composition_parameters::PublicationSpec {
                    target: library::model::authoring::CompositionParameterTarget::ItemProperty {
                        item_id: item.id,
                        property_key: key.to_string(),
                    },
                    default_value: publish_default,
                    suggested_name: format!("{} {label}", item.name),
                },
            );
            (finished, mode_action, edited_value)
        })
        .inner;
    if finished && edited_value != model_value {
        let result = local_time.clone().and_then(|time| {
            property_authoring::commit_authored_value(
                service,
                AuthoringPropertyOwner::Item(item.id),
                key,
                authored,
                edited_value.clone(),
                time,
            )
        });
        if let Err(error) = result {
            state.error = Some(error);
        }
    }
    if let Some(action) = mode_action {
        let result = local_time.and_then(|time| {
            property_authoring::apply_authored_mode_action(
                service,
                AuthoringPropertyOwner::Item(item.id),
                key,
                authored,
                edited_value,
                time,
                action,
            )
        });
        if let Err(error) = result {
            state.error = Some(error);
        } else {
            state.status = format!("{label}: {}", mode_action_label(action));
        }
    }
    if authored.is_some_and(|property| property.evaluator == "expression") {
        expression_source(
            ui,
            state,
            service,
            item,
            key,
            authored,
            &format!("item:{}:{key}", item.id),
        );
    }
    value_provenance(
        ui,
        item.authored_properties
            .get(key)
            .is_some_and(|property| property.evaluator == "keyframe"),
        false,
    );
}

fn show_text_content_property(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    spec: ItemPropertySpec<'_>,
) {
    let ItemPropertySpec {
        key,
        label,
        default,
        definition,
        suffix,
        speed,
    } = spec;
    let draft_key = format!("source:{key}");
    let model_value = default.clone();
    let (finished, edited_value) = ui
        .horizontal(|ui| {
            let (finished, edited_value, publish_default) = {
                let value = state
                    .inspector
                    .property_values
                    .entry(draft_key)
                    .or_insert(default);
                let result = property_row(
                    ui,
                    value,
                    &project.palette,
                    PropertyRowSpec {
                        control_id: &format!("item:{}:{key}", item.id),
                        label,
                        definition,
                        suffix,
                        speed,
                        mode_state: PropertyModeState::constant(0.0),
                        allow_keyframe: false,
                        keyframe_disabled_reason: Some(
                            "Text Content is source data; animate it after converting to a Node Clip",
                        ),
                        allow_expression: false,
                    },
                );
                (result.finished, value.clone(), value.clone())
            };
            composition_parameters::publication_icon(
                ui,
                project,
                state,
                service,
                item,
                composition_parameters::PublicationSpec {
                    target: library::model::authoring::CompositionParameterTarget::TextContent {
                        item_id: item.id,
                    },
                    default_value: publish_default,
                    suggested_name: format!("{} Text", item.name),
                },
            );
            (finished, edited_value)
        })
        .inner;
    if finished && edited_value != model_value {
        let PropertyValue::String(text) = edited_value else {
            state.error = Some("Text Content editor produced a non-String value".to_string());
            return;
        };
        if let Err(error) = service.set_text(item.id, text) {
            state.error = Some(error.to_string());
        }
    }
    value_provenance(ui, false, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::property::PropertyUiType;

    #[test]
    fn direct_text_uses_the_same_typed_controls_as_its_converted_module_interface() {
        let properties = text_source_properties("Authored");
        assert_eq!(
            properties
                .iter()
                .map(|property| (property.key.as_str(), property.label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("text", "Content"),
                ("font_family", "Font"),
                ("size", "Font Size"),
            ]
        );
        assert!(matches!(
            properties[1]
                .definition
                .as_ref()
                .map(library::model::property::PropertyDefinition::ui_type),
            Some(PropertyUiType::Font)
        ));
        assert_eq!(
            properties[2].default,
            PropertyValue::from(library::plugin::entity_converter::DEFAULT_TIMELINE_TEXT_SIZE)
        );
    }
}
