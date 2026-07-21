use library::model::property::{PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue};

pub(super) fn inferred_property_definitions(
    properties: &PropertyMap,
    _current_time: f64,
) -> Vec<PropertyDefinition> {
    let mut entries: Vec<_> = properties.iter().collect();
    entries.sort_by_key(|(name, _)| *name);
    entries
        .into_iter()
        .filter_map(|(name, property)| {
            let value = property.value()?.clone();
            let ui_type = match &value {
                PropertyValue::Number(_) => PropertyUiType::Float {
                    min: -1_000_000.0,
                    max: 1_000_000.0,
                    step: 0.1,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                PropertyValue::Integer(_) => PropertyUiType::Integer {
                    min: i64::MIN,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                PropertyValue::String(text) => {
                    if text.contains('\n')
                        || matches!(name.as_str(), "text" | "path" | "shader" | "code")
                    {
                        PropertyUiType::MultilineText
                    } else {
                        PropertyUiType::Text
                    }
                }
                PropertyValue::Boolean(_) => PropertyUiType::Bool,
                PropertyValue::Vec2(_) => PropertyUiType::vec2(""),
                PropertyValue::Vec3(_) => PropertyUiType::vec3(""),
                PropertyValue::Vec4(_) => PropertyUiType::vec4(""),
                PropertyValue::ColorValue(_) | PropertyValue::Color(_) => PropertyUiType::Color,
                PropertyValue::Path(_) | PropertyValue::Array(_) | PropertyValue::Map(_) => {
                    return None;
                }
            };
            Some(PropertyDefinition::new(
                name,
                ui_type,
                &property_label(name),
                value,
            ))
        })
        .collect()
}

pub(super) fn property_label(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
