use crate::model::project::connection::PortDefinition;
use crate::model::project::property::{
    PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue,
};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use uuid::Uuid;

pub const CLIP_START_TIME_PROPERTY: &str = "start_time";
pub const CLIP_DURATION_PROPERTY: &str = "duration";
pub const CLIP_TRIM_IN_PROPERTY: &str = "trim_in";
pub const CLIP_TIME_STRETCH_PROPERTY: &str = "time_stretch";
pub const TIME_MODULO_PERIOD_PROPERTY: &str = "period";

static CLIP_TIMING_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 4]> = LazyLock::new(|| {
    [
        PropertyDefinition::new(
            CLIP_START_TIME_PROPERTY,
            PropertyUiType::Float {
                min: 0.0,
                max: 86_400.0,
                step: 0.01,
                suffix: " s".to_string(),
                min_hard_limit: true,
                max_hard_limit: false,
            },
            "Start",
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        PropertyDefinition::new(
            CLIP_DURATION_PROPERTY,
            PropertyUiType::Float {
                min: 0.0,
                max: 86_400.0,
                step: 0.01,
                suffix: " s".to_string(),
                min_hard_limit: true,
                max_hard_limit: false,
            },
            "Duration",
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        PropertyDefinition::new(
            CLIP_TRIM_IN_PROPERTY,
            PropertyUiType::Float {
                min: 0.0,
                max: 86_400.0,
                step: 0.01,
                suffix: " s".to_string(),
                min_hard_limit: true,
                max_hard_limit: false,
            },
            "Source Start",
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        PropertyDefinition::new(
            CLIP_TIME_STRETCH_PROPERTY,
            PropertyUiType::Float {
                min: 0.0,
                max: 1_000.0,
                step: 0.01,
                suffix: "×".to_string(),
                min_hard_limit: true,
                max_hard_limit: false,
            },
            "Time Stretch",
            PropertyValue::Number(OrderedFloat(1.0)),
        ),
    ]
});

static TIME_MODULO_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        TIME_MODULO_PERIOD_PROPERTY,
        PropertyUiType::Float {
            min: 0.001,
            max: 86_400.0,
            step: 0.001,
            suffix: " s".to_string(),
            min_hard_limit: true,
            max_hard_limit: false,
        },
        "Period",
        PropertyValue::Number(OrderedFloat(1.0)),
    )]
});

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Multiply,
    Screen,
    Overlay,
}

/// A top-level timeline container owned by one Composition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default)]
    pub properties: PropertyMap,
    /// Rendering/timeline order for Clip containers.
    #[serde(default)]
    pub clip_ids: Vec<Uuid>,
    /// Leaf Nodes placed directly in this Track scope.
    #[serde(default)]
    pub node_ids: Vec<Uuid>,
    /// Explicit graph result for the Track image output.
    #[serde(default)]
    pub output_node_id: Option<Uuid>,
    #[serde(default)]
    pub ui_position: [f32; 2],
    #[serde(default = "default_track_ui_size")]
    pub ui_size: [f32; 2],
    #[serde(default)]
    pub ui_collapsed: bool,
}

fn default_track_ui_size() -> [f32; 2] {
    [640.0, 420.0]
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            blend_mode: BlendMode::Normal,
            properties: PropertyMap::new(),
            clip_ids: Vec::new(),
            node_ids: Vec::new(),
            output_node_id: None,
            ui_position: [0.0, 0.0],
            ui_size: default_track_ui_size(),
            ui_collapsed: false,
        }
    }
}

/// Timeline placement and isolated image container. Timing exists only here;
/// leaf Nodes never duplicate it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: Uuid,
    pub name: String,
    pub start_time: OrderedFloat<f64>,
    pub duration: OrderedFloat<f64>,
    pub trim_in: OrderedFloat<f64>,
    pub time_stretch: OrderedFloat<f64>,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub node_ids: Vec<Uuid>,
    #[serde(default)]
    pub output_node_id: Option<Uuid>,
    #[serde(default)]
    pub ui_position: [f32; 2],
    #[serde(default = "default_clip_ui_size")]
    pub ui_size: [f32; 2],
    #[serde(default)]
    pub ui_collapsed: bool,
}

fn default_clip_ui_size() -> [f32; 2] {
    [480.0, 320.0]
}

impl Clip {
    /// Canonical UI and validation metadata for the four structural Clip
    /// timing fields. These definitions are never inserted into
    /// `Clip::properties`; the fields above remain the single authority.
    /// A zero `time_stretch` is valid and freezes source time at `trim_in`.
    pub fn timing_property_definitions() -> &'static [PropertyDefinition] {
        CLIP_TIMING_PROPERTY_DEFINITIONS.as_slice()
    }

    pub fn timing_property_definition(key: &str) -> Option<&'static PropertyDefinition> {
        Self::timing_property_definitions()
            .iter()
            .find(|definition| definition.name() == key)
    }

    pub fn timing_property_value(&self, key: &str) -> Option<PropertyValue> {
        let value = match key {
            CLIP_START_TIME_PROPERTY => self.start_time,
            CLIP_DURATION_PROPERTY => self.duration,
            CLIP_TRIM_IN_PROPERTY => self.trim_in,
            CLIP_TIME_STRETCH_PROPERTY => self.time_stretch,
            _ => return None,
        };
        Some(PropertyValue::Number(value))
    }

    pub fn validate_timing_property_value(key: &str, value: &PropertyValue) -> Result<f64, String> {
        let definition = Self::timing_property_definition(key)
            .ok_or_else(|| format!("Unknown Clip timing property '{key}'"))?;
        definition.validate_value(value)?;
        let PropertyValue::Number(value) = value else {
            return Err(format!("Clip timing property '{key}' must be a number"));
        };
        Ok(value.into_inner())
    }

    pub fn update_timing_property(
        &mut self,
        key: &str,
        value: PropertyValue,
    ) -> Result<(), String> {
        let value = Self::validate_timing_property_value(key, &value)?;
        match key {
            CLIP_START_TIME_PROPERTY => self.start_time = OrderedFloat(value),
            CLIP_DURATION_PROPERTY => self.duration = OrderedFloat(value),
            CLIP_TRIM_IN_PROPERTY => self.trim_in = OrderedFloat(value),
            CLIP_TIME_STRETCH_PROPERTY => self.time_stretch = OrderedFloat(value),
            _ => return Err(format!("Unknown Clip timing property '{key}'")),
        }
        Ok(())
    }

    pub fn new(name: &str, start_time: f64, duration: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            start_time: OrderedFloat(start_time),
            duration: OrderedFloat(duration.max(0.0)),
            trim_in: OrderedFloat(0.0),
            time_stretch: OrderedFloat(1.0),
            blend_mode: BlendMode::Normal,
            properties: PropertyMap::new(),
            node_ids: Vec::new(),
            output_node_id: None,
            ui_position: [0.0, 0.0],
            ui_size: default_clip_ui_size(),
            ui_collapsed: false,
        }
    }

    pub fn end_time(&self) -> f64 {
        self.start_time.into_inner() + self.duration.into_inner()
    }

    pub fn local_time(&self, timeline_time: f64) -> f64 {
        (timeline_time - self.start_time.into_inner()) * self.time_stretch.into_inner()
            + self.trim_in.into_inner()
    }

    pub fn update_property_or_keyframe(
        &mut self,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> bool {
        if Self::timing_property_definition(property_key).is_some() {
            // Structural timing fields are static Clip placement, not
            // keyframeable PropertyMap entries.
            return easing.is_none() && self.update_timing_property(property_key, value).is_ok();
        }
        self.properties
            .update_property_or_keyframe(property_key, time, value, easing);
        true
    }
}

/// A leaf graph node. It owns media/generator/reference behavior and render
/// properties, but never timeline timing or containment.
///
/// Generic construction is intentionally unavailable: native Generators need
/// converter- and canvas-backed property definitions, while the other variants
/// have typed constructors.
///
/// ```compile_fail
/// use library::model::{GeneratorContent, Node, NodeContent};
/// let _ = Node::new("sparse", NodeContent::Generator(GeneratorContent::Text));
/// ```
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: Uuid,
    pub name: String,
    pub content: NodeContent,
    /// Authoritative authored evaluation state. Disabled Nodes produce
    /// NoOutput before resolving descriptors, properties, or upstream values.
    pub enabled: bool,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub ui_position: [f32; 2],
    /// Authoritative Node Editor presentation state. These fields deliberately
    /// have no serde fallback while the Project format is still pre-v1.
    pub ui_size: [f32; 2],
    pub ui_collapsed: bool,
}

impl Node {
    /// Creates an ordered variadic image compositor.
    pub fn new_merge(name: &str) -> Self {
        Self::with_properties(name, NodeContent::Merge, PropertyMap::new())
    }

    /// Creates a media source. Converter-backed media properties are populated
    /// by the editor factory that owns the relevant asset/canvas context.
    pub fn new_media(name: &str, content: MediaContent) -> Self {
        Self::with_properties(name, NodeContent::Media(content), PropertyMap::new())
    }

    /// Creates a composition/reference source.
    pub fn new_reference(name: &str, content: ReferenceContent) -> Self {
        Self::with_properties(name, NodeContent::Reference(content), PropertyMap::new())
    }

    /// Completion point for descriptor-backed Plugin operations. Downstream
    /// callers cannot invoke this; `OperationDescriptor::create_node` owns the
    /// public construction path and immediately materializes its definitions.
    pub(crate) fn new_plugin_operation(name: &str, content: PluginOperationContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::PluginOperation(content),
            PropertyMap::new(),
        )
    }

    /// Validated completion point for converter-backed native Generators.
    /// Public callers must use `ProjectManager::create_generator_node`, so a
    /// Generator cannot be authored without the complete converter contract.
    pub(crate) fn new_generator(
        name: &str,
        content: GeneratorContent,
        definitions: &[PropertyDefinition],
        properties: PropertyMap,
    ) -> Result<Self, String> {
        if definitions.is_empty() {
            return Err("Generator converter declared no properties".to_string());
        }

        let mut definition_names = std::collections::HashSet::with_capacity(definitions.len());
        for definition in definitions {
            definition.validate_definition().map_err(|error| {
                format!(
                    "Generator converter property '{}' has invalid metadata: {error}",
                    definition.name()
                )
            })?;
            if !definition_names.insert(definition.name()) {
                return Err(format!(
                    "Generator converter declared duplicate property '{}'",
                    definition.name()
                ));
            }
            let property = properties.get(definition.name()).ok_or_else(|| {
                format!(
                    "Generator factory omitted declared property '{}'",
                    definition.name()
                )
            })?;
            let value = property.get_static_value().ok_or_else(|| {
                format!(
                    "Generator factory property '{}' is not a constant value",
                    definition.name()
                )
            })?;
            definition.validate_value(value).map_err(|error| {
                format!(
                    "Generator factory property '{}' is invalid: {error}",
                    definition.name()
                )
            })?;
        }
        if let Some((unknown, _)) = properties
            .iter()
            .find(|(name, _)| !definition_names.contains(name.as_str()))
        {
            return Err(format!(
                "Generator factory produced undeclared property '{unknown}'"
            ));
        }

        Ok(Self::with_properties(
            name,
            NodeContent::Generator(content),
            properties,
        ))
    }

    fn with_properties(name: &str, content: NodeContent, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            content,
            enabled: true,
            blend_mode: BlendMode::Normal,
            properties,
            ui_position: [0.0, 0.0],
            ui_size: [240.0, 160.0],
            ui_collapsed: false,
        }
    }

    /// Creates a native scalar node for explicit timeline-time remapping.
    ///
    /// The dividend is deliberately not implicit: callers must wire a Number
    /// source (normally a container's internal Time output) to the `value`
    /// input. The authored period is initialized in the authoritative
    /// [`PropertyMap`] by this constructor.
    pub fn new_time_modulo(name: &str) -> Self {
        let content = ValueContent::TimeModulo;
        Self::with_properties(
            name,
            NodeContent::Value(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }

    pub fn update_property_or_keyframe(
        &mut self,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> bool {
        self.properties
            .update_property_or_keyframe(property_key, time, value, easing);
        true
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", content = "data")]
pub enum NodeContent {
    Media(MediaContent),
    Generator(GeneratorContent),
    Reference(ReferenceContent),
    /// A plugin-defined graph operation whose authored state is entirely
    /// represented by this stable identity, its persisted port contract, and
    /// [`Node::properties`]. Loading and validating a Project never requires
    /// the referenced plugin to be installed.
    PluginOperation(PluginOperationContent),
    /// Native, typed scalar operations. Inputs and outputs remain canonical
    /// Project ports; this variant does not introduce a parallel value model.
    Value(ValueContent),
    /// Ordered variadic image compositor. Input ordering lives on canonical
    /// ProjectConnection::order, never on a UI pin index.
    Merge,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueContent {
    /// Floating-point remainder used for explicit looping/time remapping.
    /// The `value` input is required and `period` may be wired or read from
    /// [`Node::properties`]. Invalid inputs produce graph `NoOutput`.
    TimeModulo,
}

impl ValueContent {
    /// Canonical authored-property metadata for this native scalar operation.
    /// Factories and inspectors consume this same definition list.
    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::TimeModulo => TIME_MODULO_PROPERTY_DEFINITIONS.as_slice(),
        }
    }
}

/// Stable, model-side identity and graph contract for a plugin operation.
///
/// All identifiers intentionally remain strings so a Project can round-trip
/// operations introduced by a newer or currently unavailable plugin. Port
/// definitions are required persisted data; omitting them is a malformed
/// pre-v1 Project rather than a request to infer them from a plugin registry.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PluginOperationContent {
    pub category: String,
    pub component_id: String,
    pub operation: String,
    pub declared_ports: Vec<PortDefinition>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct MediaContent {
    pub asset_id: Uuid,
    /// Primary visual/media stream as a zero-based global container index.
    pub stream_index: Option<usize>,
    /// Embedded audio override as a zero-based global container index.
    /// This is independent from the visual stream because they are distinct
    /// streams in a video container.
    #[serde(deserialize_with = "deserialize_required_audio_stream_index")]
    pub audio_stream_index: Option<usize>,
}

fn deserialize_required_audio_stream_index<'de, D>(
    deserializer: D,
) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<usize>::deserialize(deserializer)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum GeneratorContent {
    Shape,
    Text,
    Solid,
    SkSL,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ReferenceContent {
    pub target_id: Uuid,
    pub sync_global_time: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::Property;

    fn number_definition() -> PropertyDefinition {
        PropertyDefinition::new(
            "amount",
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Amount",
            PropertyValue::Number(OrderedFloat(50.0)),
        )
    }

    #[test]
    fn generator_completion_requires_the_exact_constant_definition_contract() -> Result<(), String>
    {
        let definitions = vec![number_definition()];
        let complete = PropertyMap::from_definitions(&definitions);
        assert!(
            Node::new_generator("complete", GeneratorContent::Solid, &definitions, complete,)
                .is_ok()
        );

        let missing = match Node::new_generator(
            "missing",
            GeneratorContent::Solid,
            &definitions,
            PropertyMap::new(),
        ) {
            Ok(_) => return Err("missing Generator property was accepted".to_string()),
            Err(error) => error,
        };
        assert!(missing.contains("omitted declared property 'amount'"));

        let mut unknown = PropertyMap::from_definitions(&definitions);
        unknown.set(
            "typo".to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
        );
        let unknown =
            match Node::new_generator("unknown", GeneratorContent::Solid, &definitions, unknown) {
                Ok(_) => return Err("undeclared Generator property was accepted".to_string()),
                Err(error) => error,
            };
        assert!(unknown.contains("undeclared property 'typo'"));

        let mut dynamic = PropertyMap::from_definitions(&definitions);
        dynamic.set(
            "amount".to_string(),
            Property::expression("time".to_string()),
        );
        let dynamic =
            match Node::new_generator("dynamic", GeneratorContent::Solid, &definitions, dynamic) {
                Ok(_) => return Err("dynamic Generator initial value was accepted".to_string()),
                Err(error) => error,
            };
        assert!(dynamic.contains("not a constant value"));

        let mut invalid = PropertyMap::from_definitions(&definitions);
        invalid.set(
            "amount".to_string(),
            Property::constant(PropertyValue::String("wrong".to_string())),
        );
        let invalid =
            match Node::new_generator("invalid", GeneratorContent::Solid, &definitions, invalid) {
                Ok(_) => return Err("invalid Generator property value was accepted".to_string()),
                Err(error) => error,
            };
        assert!(invalid.contains("is invalid"));
        Ok(())
    }

    #[test]
    fn sparse_pre_v1_generator_still_deserializes_losslessly() -> Result<(), serde_json::Error> {
        let mut sparse = Node::new_merge("persisted sparse generator");
        sparse.content = NodeContent::Generator(GeneratorContent::Text);
        let json = serde_json::to_string(&sparse)?;
        let loaded: Node = serde_json::from_str(&json)?;

        assert_eq!(loaded, sparse);
        assert!(loaded.properties.iter().next().is_none());
        Ok(())
    }
}
