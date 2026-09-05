use crate::model::Node;
use crate::model::Project;
use crate::model::frame::entity::FrameObject;
use crate::model::frame::runtime_shape::RuntimeShape;
use crate::model::frame::transform::{Position, Scale, Transform};
use crate::model::project::{Composition, EvalOutput};
use crate::model::property::{PropertyMap, PropertyValue, Vec2};
use crate::plugin::{
    EvaluationContext, PluginManager, PropertyEvaluationError, PropertyEvaluatorRegistry,
};
use std::collections::HashMap;

mod image;
mod shape;
mod sksl;
mod solid;
mod text;
mod video;

pub use crate::model::frame::runtime_shape::measure_shape_visual_bounds;
pub use image::ImageEntityConverterPlugin;
pub use shape::ShapeEntityConverterPlugin;
pub(crate) use shape::{primitive_shape_path_data, runtime_path_shape};
pub use sksl::SkSLEntityConverterPlugin;
pub use solid::SolidEntityConverterPlugin;
pub use text::measure_text_size;
pub(crate) use text::runtime_text_shape;
pub use text::{
    DEFAULT_TEXT_FONT_FAMILY, DEFAULT_TEXT_NODE_SIZE, DEFAULT_TIMELINE_TEXT_SIZE,
    TextEntityConverterPlugin, timeline_text_property_definitions,
};
pub use video::VideoEntityConverterPlugin;

/// Render-only typed values resolved from canonical Project connections.
/// These values are never persisted and never become a second authored model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedNodeInputs {
    /// Active scope metadata. Kept separate so an authored property literally
    /// named `time`, `duration`, or `fps` is never shadowed implicitly.
    pub metadata: HashMap<String, EvalOutput<PropertyValue>>,
    /// Explicit scalar/property wires keyed by their logical PropertyMap key.
    pub properties: HashMap<String, EvalOutput<PropertyValue>>,
}

impl ResolvedNodeInputs {
    pub fn from_metadata(metadata: HashMap<String, EvalOutput<PropertyValue>>) -> Self {
        Self {
            metadata,
            properties: HashMap::new(),
        }
    }
}

pub struct FrameEvaluationContext<'a> {
    pub project: &'a Project,
    pub composition: &'a Composition,
    pub property_evaluators: &'a PropertyEvaluatorRegistry,
    pub plugin_manager: &'a PluginManager,
    /// Render-time values arriving through canonical Project connections.
    /// These override same-key authored/keyframed properties without mutating
    /// the authoritative PropertyMap.
    pub resolved_inputs: Option<&'a ResolvedNodeInputs>,
}

impl<'a> FrameEvaluationContext<'a> {
    pub fn connected_input(&self, key: &str) -> Option<&PropertyValue> {
        match self
            .resolved_inputs
            .and_then(|inputs| inputs.properties.get(key))
        {
            Some(EvalOutput::Produced(value)) => Some(value),
            Some(EvalOutput::NoOutput) | None => None,
        }
    }

    pub fn connected_port(&self, key: &str) -> Option<&EvalOutput<PropertyValue>> {
        self.resolved_inputs
            .and_then(|inputs| inputs.properties.get(key))
    }

    pub fn metadata_input(&self, key: &str) -> Option<&PropertyValue> {
        match self
            .resolved_inputs
            .and_then(|inputs| inputs.metadata.get(key))
        {
            Some(EvalOutput::Produced(value)) => Some(value),
            Some(EvalOutput::NoOutput) | None => None,
        }
    }

    pub fn evaluation_fps(&self) -> f64 {
        self.metadata_input(crate::model::project::FPS_PORT)
            .and_then(|value| value.get_as::<f64>())
            .filter(|fps| fps.is_finite() && *fps > 0.0)
            .unwrap_or(self.composition.fps)
    }

    pub fn evaluation_resolution(&self) -> (u64, u64) {
        self.metadata_input(crate::model::project::RESOLUTION_PORT)
            .and_then(|value| value.get_as::<Vec2>())
            .map(|value| {
                (
                    value.x.into_inner().max(1.0) as u64,
                    value.y.into_inner().max(1.0) as u64,
                )
            })
            .unwrap_or((self.composition.width, self.composition.height))
    }

    fn evaluate_key(&self, props: &PropertyMap, key: &str, time: f64) -> Option<PropertyValue> {
        match self.connected_port(key) {
            Some(EvalOutput::Produced(value)) => Some(value.clone()),
            Some(EvalOutput::NoOutput) => {
                log::debug!("Input {key} produced NoOutput");
                None
            }
            None => props.get(key).and_then(|property| {
                self.evaluate_property_value(property, props, time)
                    .inspect_err(|error| log::error!("{error}"))
                    .ok()
            }),
        }
    }

    pub fn evaluate_property_value(
        &self,
        property: &crate::model::property::Property,
        properties: &PropertyMap,
        time: f64,
    ) -> Result<PropertyValue, PropertyEvaluationError> {
        let ctx = EvaluationContext::new(
            properties,
            self.evaluation_fps(),
            self.evaluation_resolution(),
        );
        self.property_evaluators.evaluate(property, time, &ctx)
    }

    pub fn evaluate_number(&self, props: &PropertyMap, key: &str, time: f64, default: f64) -> f64 {
        self.evaluate_key(props, key, time)
            .and_then(|value| value.get_as::<f64>())
            .unwrap_or(default)
    }

    pub fn evaluate_vec2(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: [f64; 2],
    ) -> [f64; 2] {
        if let Some(val) = self.evaluate_key(props, key, time)
            && let Some(v) = val.get_as::<Vec2>()
        {
            return [v.x.into_inner(), v.y.into_inner()];
        }
        default
    }

    pub fn evaluate_vec2_components(
        &self,
        props: &PropertyMap,
        key_main: &str,
        key_x: &str,
        key_y: &str,
        time: f64,
        default: (f64, f64),
    ) -> (f64, f64) {
        // Try main key first (Vec2)
        if let Some(val) = self.evaluate_key(props, key_main, time)
            && let Some(v) = val.get_as::<Vec2>()
        {
            return (v.x.into_inner(), v.y.into_inner());
        }

        // Fallback to components
        let x = self.evaluate_number(props, key_x, time, default.0);
        let y = self.evaluate_number(props, key_y, time, default.1);
        (x, y)
    }

    pub fn require_string(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        context: &str,
    ) -> Option<String> {
        if let Some(val) = self.evaluate_key(props, key, time)
            && let Some(s) = val.get_as::<String>()
        {
            return Some(s);
        }
        log::warn!(
            "Missing or invalid string property '{}' for {}",
            key,
            context
        );
        None
    }

    pub fn require_color(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        context: &str,
    ) -> Option<crate::model::frame::color::Color> {
        if let Some(value) = self.evaluate_key(props, key, time)
            && let Some(color) = value.get_as::<crate::model::frame::color::Color>()
        {
            return Some(color);
        }
        log::warn!("Missing or invalid color property '{key}' for {context}");
        None
    }

    /// Resolve a canonical graph color without inventing a legacy fallback.
    /// Conversion to a renderer-specific pixel representation belongs to the
    /// consumer that crosses that explicit boundary.
    pub fn require_color_value(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        context: &str,
    ) -> Option<crate::model::property::ColorValue> {
        if let Some(value) = self.evaluate_key(props, key, time) {
            return match value {
                PropertyValue::ColorValue(color) => Some(color),
                // Explicit, lossless read adapter for persisted pre-v1 nodes.
                // New descriptors and authoring paths never emit this type.
                PropertyValue::Color(color) => Some(
                    crate::model::property::ColorValue::from_straight_srgba8(&color),
                ),
                _ => {
                    log::warn!("Missing or invalid canonical color property '{key}' for {context}");
                    None
                }
            };
        }
        log::warn!("Missing or invalid canonical color property '{key}' for {context}");
        None
    }

    /// Resolve canonical path geometry. Existing pre-v1 Shape Nodes may still
    /// contain an SVG string; that legacy representation is accepted only at
    /// this explicit read boundary and immediately decoded to `PathValue`.
    pub fn require_path_value(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        context: &str,
    ) -> Option<crate::model::path::PathValue> {
        let value = self.evaluate_key(props, key, time)?;
        match value {
            PropertyValue::Path(path) => Some(path),
            PropertyValue::String(svg) => crate::model::path::parse_legacy_svg_path_data(&svg)
                .inspect_err(|error| {
                    log::warn!("Invalid legacy SVG property '{key}' for {context}: {error}");
                })
                .ok(),
            _ => {
                log::warn!("Missing or invalid canonical path property '{key}' for {context}");
                None
            }
        }
    }

    pub fn optional_string(&self, props: &PropertyMap, key: &str, time: f64) -> Option<String> {
        self.evaluate_key(props, key, time)
            .and_then(|value| value.get_as::<String>())
    }

    pub fn optional_bool(&self, props: &PropertyMap, key: &str, time: f64) -> Option<bool> {
        self.evaluate_key(props, key, time)
            .and_then(|value| value.get_as::<bool>())
    }

    pub fn build_transform(&self, props: &PropertyMap, time: f64) -> Transform {
        let position = self.evaluate_vec2(props, "position", time, [0.0, 0.0]);
        let anchor = self.evaluate_vec2(props, "anchor", time, [0.0, 0.0]);
        let scale = self.evaluate_vec2(props, "scale", time, [100.0, 100.0]);
        let rotation = self.evaluate_number(props, "rotation", time, 0.0);
        let opacity = self.evaluate_number(props, "opacity", time, 100.0);

        Transform {
            position: Position {
                x: position[0],
                y: position[1],
            },
            anchor: Position {
                x: anchor[0],
                y: anchor[1],
            },
            scale: Scale {
                x: scale[0] / 100.0,
                y: scale[1] / 100.0,
            },
            rotation,
            opacity: opacity / 100.0,
        }
    }

    /// Evaluates every property declared by an operation descriptor from its
    /// authoritative PropertyMap plus explicit scalar wires. Missing,
    /// NoOutput, or invalid values keep the operation at NoOutput rather than
    /// letting an individual plugin silently substitute a fallback.
    pub fn evaluate_operation_properties(
        &self,
        definitions: &[crate::model::property::PropertyDefinition],
        properties: &PropertyMap,
        time: f64,
        operation_label: &str,
    ) -> Option<HashMap<String, PropertyValue>> {
        let mut evaluated = HashMap::with_capacity(definitions.len());
        for definition in definitions {
            let Some(mut value) = self.evaluate_key(properties, definition.name(), time) else {
                log::warn!(
                    "{operation_label} property {} is missing or produced NoOutput",
                    definition.name()
                );
                return None;
            };
            if matches!(
                definition.ui_type(),
                crate::model::property::PropertyUiType::ColorValue
            ) && let PropertyValue::Color(color) = &value
            {
                // Persisted pre-v1 Style values are adapted losslessly at the
                // read boundary. Authoritative Project state is not mutated.
                value = PropertyValue::ColorValue(
                    crate::model::property::ColorValue::from_straight_srgba8(color),
                );
            }
            if let Err(error) = definition.validate_value(&value) {
                log::warn!(
                    "{operation_label} property {} evaluated to an invalid value: {}",
                    definition.name(),
                    error
                );
                return None;
            }
            evaluated.insert(definition.name().to_string(), value);
        }
        Some(evaluated)
    }

    /// Builds one descriptor-backed Effect after applying the shared
    /// operation-property validation contract.
    pub fn build_operation_effect(
        &self,
        effect_type: &str,
        definitions: &[crate::model::property::PropertyDefinition],
        properties: &PropertyMap,
        time: f64,
    ) -> Option<crate::model::frame::effect::ImageEffect> {
        let evaluated = self.evaluate_operation_properties(
            definitions,
            properties,
            time,
            &format!("Effect {effect_type}"),
        )?;
        Some(crate::model::frame::effect::ImageEffect {
            effect_type: effect_type.to_string(),
            properties: evaluated,
        })
    }

    pub fn evaluate_color(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::color::Color,
    ) -> crate::model::frame::color::Color {
        use crate::model::frame::color::Color;
        if let Some(val) = self.evaluate_key(props, key, time)
            && let Some(c) = val.get_as::<Color>()
        {
            return c;
        }
        default
    }

    pub fn evaluate_cap_type(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::draw_type::CapType,
    ) -> crate::model::frame::draw_type::CapType {
        use crate::model::frame::draw_type::CapType;
        if let Some(val) = self.evaluate_key(props, key, time)
            && let Some(s) = val.get_as::<String>()
        {
            return match s.to_lowercase().as_str() {
                "round" => CapType::Round,
                "square" => CapType::Square,
                "butt" => CapType::Butt,
                _ => {
                    log::warn!("Unknown CapType: {}", s);
                    default
                }
            };
        }
        default
    }

    pub fn evaluate_join_type(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::draw_type::JoinType,
    ) -> crate::model::frame::draw_type::JoinType {
        use crate::model::frame::draw_type::JoinType;
        if let Some(val) = self.evaluate_key(props, key, time)
            && let Some(s) = val.get_as::<String>()
        {
            return match s.to_lowercase().as_str() {
                "round" => JoinType::Round,
                "bevel" => JoinType::Bevel,
                "miter" => JoinType::Miter,
                _ => {
                    log::warn!("Unknown JoinType: {}", s);
                    default
                }
            };
        }
        default
    }

    pub fn evaluate_number_array(&self, props: &PropertyMap, key: &str, time: f64) -> Vec<f64> {
        use crate::model::property::PropertyValue;
        if let Some(val) = self.evaluate_key(props, key, time) {
            if let Some(arr) = val.get_as::<Vec<PropertyValue>>() {
                return arr.iter().filter_map(|v| v.get_as::<f64>()).collect();
            }
            if let Some(s) = val.get_as::<String>() {
                return s
                    .split(&[',', ' '][..])
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
            }
        }
        Vec::new()
    }
}

/// Trait for entity converter plugins.
pub trait EntityConverterPlugin: crate::plugin::Plugin + Send + Sync {
    fn supports_kind(&self, kind: &str) -> bool;

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        layer: &Node,
        time: f64,
    ) -> Option<FrameObject>;

    /// Evaluate a vector/typographic generator without rasterizing it. Only
    /// Shape-producing converters override this; Image producers return None.
    fn convert_shape(
        &self,
        _evaluator: &FrameEvaluationContext,
        _node: &Node,
        _time: f64,
    ) -> Option<RuntimeShape> {
        None
    }

    fn get_bounds(
        &self,
        _evaluator: &FrameEvaluationContext,
        _node: &Node,
        _time: f64,
    ) -> Option<(f32, f32, f32, f32)> {
        None
    }

    fn get_property_definitions(
        &self,
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        Vec::new()
    }

    fn plugin_type(&self) -> crate::plugin::PluginCategory {
        crate::plugin::PluginCategory::EntityConverter
    }
}
