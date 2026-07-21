use std::collections::HashSet;

use ordered_float::OrderedFloat;
use ruvie_plugin_api::{
    ComponentDescriptorV1, DECORATOR_CATEGORY, DECORATOR_EVALUATE_V1, DECORATOR_EVALUATE_V2,
    EFFECT_CATEGORY, EFFECT_PROCESS_CPU_RGBA8_V1, EFFECTOR_CATEGORY, EFFECTOR_EVALUATE_V1,
    LOADER_CATEGORY, LOADER_LOAD_CPU_RGBA8_V1, LOADER_OPEN_V1, PROPERTY_CATEGORY,
    PROPERTY_EVALUATE_V1, PluginDescriptorV1, PropertyUiV1, STYLE_CATEGORY, STYLE_EVALUATE_V1,
};

use super::RUNTIME_EFFECT_TIME_PROPERTY;
use super::adapters::RuntimeDecoratorProtocol;
use super::property_wire::property_output_default;
use crate::error::LibraryError;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4};
pub(super) fn validate_descriptor(descriptor: &PluginDescriptorV1) -> Result<(), LibraryError> {
    if descriptor.name.trim().is_empty()
        || descriptor.vendor.trim().is_empty()
        || descriptor.version.trim().is_empty()
    {
        return Err(LibraryError::Plugin(
            "Runtime plugin descriptor name, vendor, and version must be non-empty".to_string(),
        ));
    }
    if descriptor.components.is_empty() {
        return Err(LibraryError::Plugin(
            "Runtime plugin descriptor has no components".to_string(),
        ));
    }
    for component in &descriptor.components {
        if component.id.trim().is_empty()
            || component.name.trim().is_empty()
            || component.category.trim().is_empty()
            || component.version.trim().is_empty()
        {
            return Err(LibraryError::Plugin(
                "Runtime plugin component id, name, category, and version must be non-empty"
                    .to_string(),
            ));
        }
        match component.category.as_str() {
            EFFECTOR_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == EFFECTOR_EVALUATE_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime effector '{}' does not declare {EFFECTOR_EVALUATE_V1}",
                        component.id
                    )));
                }
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime effector '{}' must not declare output_default",
                        component.id
                    )));
                }
            }
            PROPERTY_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == PROPERTY_EVALUATE_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime property '{}' does not declare {PROPERTY_EVALUATE_V1}",
                        component.id
                    )));
                }
                let _ = property_output_default(component)?;
            }
            STYLE_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == STYLE_EVALUATE_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Style '{}' does not declare {STYLE_EVALUATE_V1}",
                        component.id
                    )));
                }
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Style '{}' must not declare output_default",
                        component.id
                    )));
                }
            }
            DECORATOR_CATEGORY => {
                let Some(protocol) = RuntimeDecoratorProtocol::negotiate(component) else {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Decorator '{}' does not declare {DECORATOR_EVALUATE_V2} or {DECORATOR_EVALUATE_V1}",
                        component.id
                    )));
                };
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Decorator '{}' must not declare output_default",
                        component.id
                    )));
                }
                const BACKPLATE_APPEARANCE_PROPERTIES: &[&str] = &[
                    "color",
                    "opacity",
                    "stroke",
                    "shape",
                    "radius",
                    "corner_radius",
                ];
                if protocol == RuntimeDecoratorProtocol::V2
                    && let Some(property) = component.properties.iter().find(|property| {
                        BACKPLATE_APPEARANCE_PROPERTIES.contains(&property.name.as_str())
                    })
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Decorator v2 '{}' declares Backplate appearance property '{}'; Backplate emits geometry only and appearance belongs to Style",
                        component.id, property.name
                    )));
                }
            }
            EFFECT_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == EFFECT_PROCESS_CPU_RGBA8_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Effect '{}' does not declare {EFFECT_PROCESS_CPU_RGBA8_V1}",
                        component.id
                    )));
                }
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Effect '{}' must not declare output_default",
                        component.id
                    )));
                }
                if component
                    .properties
                    .iter()
                    .any(|property| property.name == RUNTIME_EFFECT_TIME_PROPERTY)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Effect '{}' reserves property '{}' for per-frame render time and must not declare it as instance config",
                        component.id, RUNTIME_EFFECT_TIME_PROPERTY
                    )));
                }
            }
            LOADER_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == LOADER_OPEN_V1)
                    || !component
                        .operations
                        .iter()
                        .any(|operation| operation == LOADER_LOAD_CPU_RGBA8_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Loader '{}' must declare {LOADER_OPEN_V1} and {LOADER_LOAD_CPU_RGBA8_V1}",
                        component.id
                    )));
                }
                if component.output_default.is_some() || !component.properties.is_empty() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Loader '{}' must not declare properties or output_default",
                        component.id
                    )));
                }
            }
            unsupported => {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}/{}' uses category '{unsupported}', but ABI v1 integrates only '{EFFECTOR_CATEGORY}', '{PROPERTY_CATEGORY}', '{STYLE_CATEGORY}', '{DECORATOR_CATEGORY}', '{EFFECT_CATEGORY}', and '{LOADER_CATEGORY}'; the entire bundle was rejected",
                    descriptor.name, component.id
                )));
            }
        }
        let mut names = HashSet::new();
        for property in &component.properties {
            if property.name.trim().is_empty() || property.label.trim().is_empty() {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}' has an empty property name or label",
                    component.id
                )));
            }
            if !names.insert(&property.name) {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}' repeats property '{}'",
                    component.id, property.name
                )));
            }
        }
        let _ = property_definitions(component)?;
    }
    Ok(())
}

pub(super) fn property_definitions(
    component: &ComponentDescriptorV1,
) -> Result<Vec<PropertyDefinition>, LibraryError> {
    component
        .properties
        .iter()
        .map(|definition| {
            let ui_type = match &definition.ui {
                PropertyUiV1::Float {
                    min,
                    max,
                    step,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    validate_float_ui_range(
                        &component.id,
                        &definition.name,
                        "float",
                        *min,
                        *max,
                        *step,
                    )?;
                    PropertyUiType::Float {
                        min: *min,
                        max: *max,
                        step: *step,
                        suffix: suffix.clone(),
                        min_hard_limit: *min_hard_limit,
                        max_hard_limit: *max_hard_limit,
                    }
                }
                PropertyUiV1::Integer {
                    min,
                    max,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    if min > max {
                        return Err(LibraryError::Plugin(format!(
                            "Runtime property '{}.{}' has an invalid integer range",
                            component.id, definition.name
                        )));
                    }
                    PropertyUiType::Integer {
                        min: *min,
                        max: *max,
                        suffix: suffix.clone(),
                        min_hard_limit: *min_hard_limit,
                        max_hard_limit: *max_hard_limit,
                    }
                }
                PropertyUiV1::Color => PropertyUiType::Color,
                PropertyUiV1::Text => PropertyUiType::Text,
                PropertyUiV1::MultilineText => PropertyUiType::MultilineText,
                PropertyUiV1::Bool => PropertyUiType::Bool,
                PropertyUiV1::Vec2 {
                    min,
                    max,
                    step,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    validate_float_ui_range(
                        &component.id,
                        &definition.name,
                        "vec2 component",
                        *min,
                        *max,
                        *step,
                    )?;
                    PropertyUiType::vec2_with_range(
                        *min,
                        *max,
                        *step,
                        suffix.clone(),
                        *min_hard_limit,
                        *max_hard_limit,
                    )
                }
                PropertyUiV1::Vec3 {
                    min,
                    max,
                    step,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    validate_float_ui_range(
                        &component.id,
                        &definition.name,
                        "vec3 component",
                        *min,
                        *max,
                        *step,
                    )?;
                    PropertyUiType::vec3_with_range(
                        *min,
                        *max,
                        *step,
                        suffix.clone(),
                        *min_hard_limit,
                        *max_hard_limit,
                    )
                }
                PropertyUiV1::Vec4 {
                    min,
                    max,
                    step,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    validate_float_ui_range(
                        &component.id,
                        &definition.name,
                        "vec4 component",
                        *min,
                        *max,
                        *step,
                    )?;
                    PropertyUiType::vec4_with_range(
                        *min,
                        *max,
                        *step,
                        suffix.clone(),
                        *min_hard_limit,
                        *max_hard_limit,
                    )
                }
                PropertyUiV1::Dropdown { options } => {
                    let unique = options.iter().collect::<HashSet<_>>();
                    if options.is_empty()
                        || options.iter().any(|option| option.is_empty())
                        || unique.len() != options.len()
                    {
                        return Err(LibraryError::Plugin(format!(
                            "Runtime property '{}.{}' has invalid dropdown options",
                            component.id, definition.name
                        )));
                    }
                    PropertyUiType::Dropdown {
                        options: options.clone(),
                    }
                }
                PropertyUiV1::Font => PropertyUiType::Font,
            };
            let default_value = strict_default_value(component, definition)?;
            if let PropertyUiType::Dropdown { options } = &ui_type
                && let PropertyValue::String(value) = &default_value
                && !options.contains(value)
            {
                return Err(LibraryError::Plugin(format!(
                    "Runtime property '{}.{}' default is not a dropdown option",
                    component.id, definition.name
                )));
            }
            let property_definition = PropertyDefinition::new(
                &definition.name,
                ui_type,
                &definition.label,
                default_value,
            );
            property_definition
                .validate_value(property_definition.default_value())
                .map_err(|error| {
                    LibraryError::Plugin(format!(
                        "Runtime property '{}.{}' has an invalid default: {error}",
                        component.id, definition.name
                    ))
                })?;
            Ok(property_definition)
        })
        .collect()
}

fn validate_float_ui_range(
    component_id: &str,
    property_name: &str,
    kind: &str,
    min: f64,
    max: f64,
    step: f64,
) -> Result<(), LibraryError> {
    if !min.is_finite() || !max.is_finite() || !step.is_finite() || min > max || step <= 0.0 {
        return Err(LibraryError::Plugin(format!(
            "Runtime property '{component_id}.{property_name}' has an invalid {kind} range"
        )));
    }
    Ok(())
}

fn strict_default_value(
    component: &ComponentDescriptorV1,
    definition: &ruvie_plugin_api::PropertyDefinitionV1,
) -> Result<PropertyValue, LibraryError> {
    let invalid = |detail: &str| {
        LibraryError::Plugin(format!(
            "Runtime property '{}.{}' has an invalid default: {detail}",
            component.id, definition.name
        ))
    };
    match &definition.ui {
        PropertyUiV1::Float { .. } => definition
            .default
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| PropertyValue::Number(OrderedFloat(value)))
            .ok_or_else(|| invalid("expected a finite JSON number")),
        PropertyUiV1::Integer { .. } => definition
            .default
            .as_i64()
            .map(PropertyValue::Integer)
            .ok_or_else(|| invalid("expected a JSON integer representable as i64")),
        PropertyUiV1::Color => {
            let object = exact_object(&definition.default, &["r", "g", "b", "a"])
                .ok_or_else(|| invalid("expected exactly integer fields r, g, b, and a"))?;
            let channel = |name: &str| {
                object
                    .get(name)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| invalid("color channels must be integers in 0..=255"))
            };
            Ok(PropertyValue::Color(crate::model::frame::color::Color {
                r: channel("r")?,
                g: channel("g")?,
                b: channel("b")?,
                a: channel("a")?,
            }))
        }
        PropertyUiV1::Text
        | PropertyUiV1::MultilineText
        | PropertyUiV1::Dropdown { .. }
        | PropertyUiV1::Font => definition
            .default
            .as_str()
            .map(|value| PropertyValue::String(value.to_string()))
            .ok_or_else(|| invalid("expected a JSON string")),
        PropertyUiV1::Bool => definition
            .default
            .as_bool()
            .map(PropertyValue::Boolean)
            .ok_or_else(|| invalid("expected a JSON boolean")),
        PropertyUiV1::Vec2 { .. } => {
            let values = strict_vector(&definition.default, &["x", "y"])
                .ok_or_else(|| invalid("expected exactly finite number fields x and y"))?;
            Ok(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(values[0]),
                y: OrderedFloat(values[1]),
            }))
        }
        PropertyUiV1::Vec3 { .. } => {
            let values = strict_vector(&definition.default, &["x", "y", "z"])
                .ok_or_else(|| invalid("expected exactly finite number fields x, y, and z"))?;
            Ok(PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(values[0]),
                y: OrderedFloat(values[1]),
                z: OrderedFloat(values[2]),
            }))
        }
        PropertyUiV1::Vec4 { .. } => {
            let values = strict_vector(&definition.default, &["x", "y", "z", "w"])
                .ok_or_else(|| invalid("expected exactly finite number fields x, y, z, and w"))?;
            Ok(PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(values[0]),
                y: OrderedFloat(values[1]),
                z: OrderedFloat(values[2]),
                w: OrderedFloat(values[3]),
            }))
        }
    }
}

fn exact_object<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    let object = value.as_object()?;
    (object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)))
        .then_some(object)
}

fn strict_vector(value: &serde_json::Value, keys: &[&str]) -> Option<Vec<f64>> {
    let object = exact_object(value, keys)?;
    keys.iter()
        .map(|key| object.get(*key)?.as_f64().filter(|value| value.is_finite()))
        .collect()
}
