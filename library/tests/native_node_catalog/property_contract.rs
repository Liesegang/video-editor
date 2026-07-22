use std::collections::BTreeMap;

use library::model::frame::color::Color;
use library::model::property::{
    ColorSpaceRef, ColorValue, PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4,
};
use ordered_float::OrderedFloat;

#[derive(Clone, Debug, Default)]
pub(super) struct NodeListPropertyMetadata {
    pub(super) label: Option<String>,
    pub(super) ui_type: Option<String>,
    pub(super) default: Option<String>,
    pub(super) min: Option<String>,
    pub(super) max: Option<String>,
    pub(super) step: Option<String>,
    pub(super) suffix: Option<String>,
    pub(super) min_hard_limit: Option<String>,
    pub(super) max_hard_limit: Option<String>,
    pub(super) options: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PropertyContract {
    key: String,
    label: String,
    ui_type: PropertyUiType,
    default: PropertyValue,
}

impl PropertyContract {
    pub(super) fn from_definition(definition: &PropertyDefinition) -> Self {
        Self {
            key: definition.name().to_string(),
            label: definition.label().to_string(),
            ui_type: definition.ui_type().clone(),
            default: definition.default_value().clone(),
        }
    }
}

impl NodeListPropertyMetadata {
    pub(super) fn contract(
        &self,
        key: &str,
        label: &str,
        context: &str,
    ) -> Result<PropertyContract, String> {
        if self
            .label
            .as_deref()
            .is_some_and(|metadata| metadata != label)
        {
            return Err(format!(
                "{context}: property_metadata label does not match the property port label"
            ));
        }
        let ui_type_name = self.required("ui_type", &self.ui_type, context)?;
        let ui_type = match ui_type_name {
            "Float" => {
                self.reject("options", &self.options, context)?;
                let (min, max, step, suffix, min_hard_limit, max_hard_limit) =
                    self.float_metadata(context)?;
                PropertyUiType::Float {
                    min,
                    max,
                    step,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                }
            }
            "Integer" => {
                self.reject("step", &self.step, context)?;
                self.reject("options", &self.options, context)?;
                PropertyUiType::Integer {
                    min: self.parse("min", &self.min, context)?,
                    max: self.parse("max", &self.max, context)?,
                    suffix: self.required("suffix", &self.suffix, context)?.to_string(),
                    min_hard_limit: self.parse("min_hard_limit", &self.min_hard_limit, context)?,
                    max_hard_limit: self.parse("max_hard_limit", &self.max_hard_limit, context)?,
                }
            }
            "Vec2" => self.vector_ui(context, |metadata| PropertyUiType::Vec2 {
                min: metadata.0,
                max: metadata.1,
                step: metadata.2,
                suffix: metadata.3,
                min_hard_limit: metadata.4,
                max_hard_limit: metadata.5,
            })?,
            "Vec3" => self.vector_ui(context, |metadata| PropertyUiType::Vec3 {
                min: metadata.0,
                max: metadata.1,
                step: metadata.2,
                suffix: metadata.3,
                min_hard_limit: metadata.4,
                max_hard_limit: metadata.5,
            })?,
            "Vec4" => self.vector_ui(context, |metadata| PropertyUiType::Vec4 {
                min: metadata.0,
                max: metadata.1,
                step: metadata.2,
                suffix: metadata.3,
                min_hard_limit: metadata.4,
                max_hard_limit: metadata.5,
            })?,
            "Dropdown" => {
                self.reject_numeric_metadata(context)?;
                PropertyUiType::Dropdown {
                    options: parse_inline_list(
                        self.required("options", &self.options, context)?,
                        context,
                    )?,
                }
            }
            "ColorValue" => {
                self.reject_non_scalar_metadata(context)?;
                PropertyUiType::ColorValue
            }
            "Color" => {
                self.reject_non_scalar_metadata(context)?;
                PropertyUiType::Color
            }
            "Text" => {
                self.reject_non_scalar_metadata(context)?;
                PropertyUiType::Text
            }
            "MultilineText" => {
                self.reject_non_scalar_metadata(context)?;
                PropertyUiType::MultilineText
            }
            "Bool" => {
                self.reject_non_scalar_metadata(context)?;
                PropertyUiType::Bool
            }
            "Font" => {
                self.reject_non_scalar_metadata(context)?;
                PropertyUiType::Font
            }
            other => return Err(format!("{context}: unsupported property ui_type {other}")),
        };
        let default = parse_property_default(
            &ui_type,
            self.required("default", &self.default, context)?,
            context,
        )?;
        Ok(PropertyContract {
            key: key.to_string(),
            label: label.to_string(),
            ui_type,
            default,
        })
    }

    fn required<'a>(
        &self,
        field: &str,
        value: &'a Option<String>,
        context: &str,
    ) -> Result<&'a str, String> {
        value
            .as_deref()
            .ok_or_else(|| format!("{context}: property_metadata is missing {field}"))
    }

    fn parse<T: std::str::FromStr>(
        &self,
        field: &str,
        value: &Option<String>,
        context: &str,
    ) -> Result<T, String> {
        let value = self.required(field, value, context)?;
        value
            .parse::<T>()
            .map_err(|_| format!("{context}: invalid property_metadata {field} {value:?}"))
    }

    fn reject(&self, field: &str, value: &Option<String>, context: &str) -> Result<(), String> {
        if value.is_some() {
            Err(format!(
                "{context}: property_metadata {field} is not valid for this ui_type"
            ))
        } else {
            Ok(())
        }
    }

    fn float_metadata(&self, context: &str) -> Result<(f64, f64, f64, String, bool, bool), String> {
        Ok((
            self.parse("min", &self.min, context)?,
            self.parse("max", &self.max, context)?,
            self.parse("step", &self.step, context)?,
            self.required("suffix", &self.suffix, context)?.to_string(),
            self.parse("min_hard_limit", &self.min_hard_limit, context)?,
            self.parse("max_hard_limit", &self.max_hard_limit, context)?,
        ))
    }

    fn vector_ui(
        &self,
        context: &str,
        build: impl FnOnce((f64, f64, f64, String, bool, bool)) -> PropertyUiType,
    ) -> Result<PropertyUiType, String> {
        self.reject("options", &self.options, context)?;
        self.float_metadata(context).map(build)
    }

    fn reject_numeric_metadata(&self, context: &str) -> Result<(), String> {
        for (field, value) in [
            ("min", &self.min),
            ("max", &self.max),
            ("step", &self.step),
            ("suffix", &self.suffix),
            ("min_hard_limit", &self.min_hard_limit),
            ("max_hard_limit", &self.max_hard_limit),
        ] {
            self.reject(field, value, context)?;
        }
        Ok(())
    }

    fn reject_non_scalar_metadata(&self, context: &str) -> Result<(), String> {
        self.reject_numeric_metadata(context)?;
        self.reject("options", &self.options, context)
    }
}

fn parse_inline_list(value: &str, context: &str) -> Result<Vec<String>, String> {
    let body = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("{context}: expected an inline YAML list, got {value:?}"))?;
    let values = body
        .split(',')
        .map(|item| item.trim().trim_matches(['\'', '"']).to_string())
        .collect::<Vec<_>>();
    if values.is_empty() || values.iter().any(String::is_empty) {
        return Err(format!("{context}: property options must not be empty"));
    }
    Ok(values)
}

fn parse_inline_map(value: &str, context: &str) -> Result<BTreeMap<String, String>, String> {
    let body = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| format!("{context}: expected an inline YAML map, got {value:?}"))?;
    body.split(',')
        .map(|item| {
            let (key, value) = item
                .split_once(':')
                .ok_or_else(|| format!("{context}: invalid inline YAML map item {item:?}"))?;
            Ok((
                key.trim().to_string(),
                value.trim().trim_matches(['\'', '"']).to_string(),
            ))
        })
        .collect()
}

fn map_value<'a>(
    values: &'a BTreeMap<String, String>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    values
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("{context}: default map is missing {key}"))
}

fn parse_map_number<T: std::str::FromStr>(
    values: &BTreeMap<String, String>,
    key: &str,
    context: &str,
) -> Result<T, String> {
    let value = map_value(values, key, context)?;
    value
        .parse::<T>()
        .map_err(|_| format!("{context}: invalid default component {key}: {value:?}"))
}

fn parse_property_default(
    ui_type: &PropertyUiType,
    value: &str,
    context: &str,
) -> Result<PropertyValue, String> {
    let invalid = || format!("{context}: invalid default {value:?} for {ui_type:?}");
    match ui_type {
        PropertyUiType::Float { .. } => value
            .parse::<f64>()
            .map(|value| PropertyValue::Number(OrderedFloat(value)))
            .map_err(|_| invalid()),
        PropertyUiType::Integer { .. } => value
            .parse::<i64>()
            .map(PropertyValue::Integer)
            .map_err(|_| invalid()),
        PropertyUiType::Text
        | PropertyUiType::MultilineText
        | PropertyUiType::Dropdown { .. }
        | PropertyUiType::Font => Ok(PropertyValue::String(value.to_string())),
        PropertyUiType::Bool => value
            .parse::<bool>()
            .map(PropertyValue::Boolean)
            .map_err(|_| invalid()),
        PropertyUiType::Vec2 { .. } => {
            let values = parse_inline_map(value, context)?;
            Ok(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(parse_map_number(&values, "x", context)?),
                y: OrderedFloat(parse_map_number(&values, "y", context)?),
            }))
        }
        PropertyUiType::Vec3 { .. } => {
            let values = parse_inline_map(value, context)?;
            Ok(PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(parse_map_number(&values, "x", context)?),
                y: OrderedFloat(parse_map_number(&values, "y", context)?),
                z: OrderedFloat(parse_map_number(&values, "z", context)?),
            }))
        }
        PropertyUiType::Vec4 { .. } => {
            let values = parse_inline_map(value, context)?;
            Ok(PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(parse_map_number(&values, "x", context)?),
                y: OrderedFloat(parse_map_number(&values, "y", context)?),
                z: OrderedFloat(parse_map_number(&values, "z", context)?),
                w: OrderedFloat(parse_map_number(&values, "w", context)?),
            }))
        }
        PropertyUiType::Color => {
            let values = parse_inline_map(value, context)?;
            Ok(PropertyValue::Color(Color {
                r: parse_map_number(&values, "r", context)?,
                g: parse_map_number(&values, "g", context)?,
                b: parse_map_number(&values, "b", context)?,
                a: parse_map_number(&values, "a", context)?,
            }))
        }
        PropertyUiType::ColorValue => {
            let values = parse_inline_map(value, context)?;
            let color_space = ColorSpaceRef::new(map_value(&values, "space", context)?)
                .map_err(|error| format!("{context}: invalid default color space: {error}"))?;
            let rgba = [
                parse_map_number(&values, "r", context)?,
                parse_map_number(&values, "g", context)?,
                parse_map_number(&values, "b", context)?,
                parse_map_number(&values, "a", context)?,
            ];
            ColorValue::new(color_space, rgba)
                .map(PropertyValue::ColorValue)
                .map_err(|error| format!("{context}: invalid ColorValue default: {error}"))
        }
        PropertyUiType::Path => Err(format!(
            "{context}: Path defaults need a canonical node_list.yml representation"
        )),
    }
}
