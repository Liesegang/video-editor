use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType};
use crate::model::frame::entity::StyleConfig;
use crate::model::property::{PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory};
use uuid::Uuid;

pub fn image_opacity_property_definitions() -> Vec<PropertyDefinition> {
    vec![PropertyDefinition::new(
        "opacity",
        PropertyUiType::Float {
            min: 0.0,
            max: 1.0,
            step: 0.01,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        "Opacity",
        PropertyValue::from(1.0),
    )]
}

pub trait StylePlugin: Plugin {
    /// Authoritative operation identity, ports, property metadata, and
    /// defaults for this Style producer.
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError>;

    fn properties(&self) -> Vec<PropertyDefinition> {
        match self.descriptor() {
            Ok(descriptor) => descriptor.properties().to_vec(),
            Err(error) => {
                log::error!("Invalid Style descriptor for {}: {error}", self.id());
                Vec::new()
            }
        }
    }

    /// Evaluates one explicit Style operation Node from its direct properties.
    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<StyleConfig>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Style
    }
}

/// Native raster appearance boundary. Its descriptor participates in the
/// ordinary Style registry/menu, while frame evaluation uses its typed Image
/// contract instead of asking Shape Style code to manufacture geometry.
pub struct ImageOpacityStylePlugin;

impl Plugin for ImageOpacityStylePlugin {
    fn id(&self) -> &'static str {
        crate::plugin::IMAGE_OPACITY_STYLE_COMPONENT_ID
    }

    fn name(&self) -> String {
        "Image Opacity".to_string()
    }

    fn category(&self) -> String {
        "Built-in".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl StylePlugin for ImageOpacityStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::image_opacity_style(image_opacity_property_definitions())
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        _source_id: Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<StyleConfig> {
        // Image Opacity is evaluated by the typed Image -> Image frame path.
        None
    }
}

pub struct FillStylePlugin;

fn apply_opacity(mut color: Color, opacity: f64) -> Color {
    // DrawStyle is the legacy u8 renderer boundary. Opacity is an explicit
    // Style operation at that boundary, not a conversion of Project color
    // data, so its result is intentionally quantized exactly once here.
    color.a = (f64::from(color.a) * opacity).round().clamp(0.0, 255.0) as u8;
    color
}

/// Evaluates the built-in Shape appearance boundary from values already
/// sampled by an authoring runtime. The legacy graph context and Module graph
/// therefore share the same Fill/Stroke materialization policy.
pub(crate) fn builtin_style_from_values(
    component_id: &str,
    source_id: Uuid,
    values: &std::collections::HashMap<String, PropertyValue>,
) -> Option<StyleConfig> {
    fn number(values: &std::collections::HashMap<String, PropertyValue>, key: &str) -> Option<f64> {
        match values.get(key)? {
            PropertyValue::Number(value) => Some(value.into_inner()),
            PropertyValue::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }
    fn string<'a>(
        values: &'a std::collections::HashMap<String, PropertyValue>,
        key: &str,
    ) -> Option<&'a str> {
        match values.get(key)? {
            PropertyValue::String(value) => Some(value),
            _ => None,
        }
    }
    fn color(values: &std::collections::HashMap<String, PropertyValue>) -> Option<Color> {
        match values.get("color")? {
            PropertyValue::Color(value) => Some(value.clone()),
            PropertyValue::ColorValue(value) => {
                crate::color_management::to_renderer_srgba8(value).ok()
            }
            _ => None,
        }
    }

    let opacity = number(values, "opacity")?;
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return None;
    }
    let final_color = apply_opacity(color(values)?, opacity);
    let style = match component_id {
        "fill" => {
            let offset = number(values, "offset")?;
            if !offset.is_finite() {
                return None;
            }
            DrawStyle::Fill {
                color: final_color,
                offset,
            }
        }
        "stroke" => {
            let width = number(values, "width")?;
            let offset = number(values, "offset")?;
            let miter = number(values, "miter_limit")?;
            let dash_offset = number(values, "dash_offset")?;
            if [width, offset, miter, dash_offset]
                .into_iter()
                .any(|value| !value.is_finite())
                || width < 0.0
                || miter < 0.0
            {
                return None;
            }
            let dash_array = string(values, "dash_array")?
                .split_whitespace()
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if dash_array.iter().any(|value| !value.is_finite()) {
                return None;
            }
            DrawStyle::Stroke {
                color: final_color,
                width,
                offset,
                join: match string(values, "join")? {
                    "Miter" => JoinType::Miter,
                    "Bevel" => JoinType::Bevel,
                    _ => JoinType::Round,
                },
                cap: match string(values, "cap")? {
                    "Butt" => CapType::Butt,
                    "Square" => CapType::Square,
                    _ => CapType::Round,
                },
                miter,
                dash_array,
                dash_offset,
            }
        }
        _ => return None,
    };
    Some(StyleConfig {
        id: source_id,
        style,
    })
}

impl Plugin for FillStylePlugin {
    fn id(&self) -> &'static str {
        "fill"
    }
    fn name(&self) -> String {
        "Fill".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl StylePlugin for FillStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(
            self.id(),
            self.name(),
            vec![
                PropertyDefinition::new(
                    "color",
                    PropertyUiType::ColorValue,
                    "Color",
                    PropertyValue::ColorValue(
                        crate::model::property::ColorValue::from_straight_srgba8(&Color::white()),
                    ),
                ),
                PropertyDefinition::new(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(1.0),
                ),
                PropertyDefinition::new(
                    "offset",
                    PropertyUiType::Float {
                        min: -50.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Offset",
                    PropertyValue::from(0.0),
                ),
            ],
        )
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<StyleConfig> {
        let values = context.evaluate_operation_properties(
            self.descriptor().ok()?.properties(),
            properties,
            eval_time,
            "Fill",
        )?;
        builtin_style_from_values(self.id(), source_id, &values)
    }
}

pub struct StrokeStylePlugin;
impl Plugin for StrokeStylePlugin {
    fn id(&self) -> &'static str {
        "stroke"
    }
    fn name(&self) -> String {
        "Stroke".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl StylePlugin for StrokeStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(
            self.id(),
            self.name(),
            vec![
                PropertyDefinition::new(
                    "color",
                    PropertyUiType::ColorValue,
                    "Color",
                    PropertyValue::ColorValue(
                        crate::model::property::ColorValue::from_straight_srgba8(&Color::white()),
                    ),
                ),
                PropertyDefinition::new(
                    "width",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Width",
                    PropertyValue::from(1.0),
                ),
                PropertyDefinition::new(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(1.0),
                ),
                PropertyDefinition::new(
                    "offset",
                    PropertyUiType::Float {
                        min: -50.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Offset",
                    PropertyValue::from(0.0),
                ),
                PropertyDefinition::new(
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
                ),
                PropertyDefinition::new(
                    "cap",
                    PropertyUiType::Dropdown {
                        options: vec![
                            "Butt".to_string(),
                            "Round".to_string(),
                            "Square".to_string(),
                        ],
                    },
                    "Cap",
                    PropertyValue::String("Round".to_string()),
                ),
                PropertyDefinition::new(
                    "miter_limit",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 0.1,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Miter Limit",
                    PropertyValue::from(4.0),
                ),
                PropertyDefinition::new(
                    "dash_array",
                    PropertyUiType::Text,
                    "Dash Array",
                    PropertyValue::String("".to_string()),
                ),
                PropertyDefinition::new(
                    "dash_offset",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Dash Offset",
                    PropertyValue::from(0.0),
                ),
            ],
        )
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<StyleConfig> {
        let values = context.evaluate_operation_properties(
            self.descriptor().ok()?.properties(),
            properties,
            eval_time,
            "Stroke",
        )?;
        builtin_style_from_values(self.id(), source_id, &values)
    }
}
