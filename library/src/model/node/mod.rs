use crate::model::numeric::NumericBinaryOperation;
use crate::model::project::connection::{
    FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, PortDataType,
    PortDefinition, PortExposure, PortSide,
};
use crate::model::project::property::{
    Property, PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue,
};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use uuid::Uuid;

pub const CLIP_START_TIME_PROPERTY: &str = "start_time";
pub const CLIP_DURATION_PROPERTY: &str = "duration";
pub const CLIP_TRIM_IN_PROPERTY: &str = "trim_in";
pub const CLIP_TIME_STRETCH_PROPERTY: &str = "time_stretch";

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

static FMOD_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        FMOD_DIVISOR_INPUT_PORT,
        PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            step: 0.01,
            suffix: String::new(),
            min_hard_limit: false,
            max_hard_limit: false,
        },
        "Divisor",
        PropertyValue::Number(OrderedFloat(1.0)),
    )]
});

static FMOD_PORT_DEFINITIONS: LazyLock<[PortDefinition; 3]> = LazyLock::new(|| {
    [
        PortDefinition::input(FMOD_X_INPUT_PORT, "X", PortDataType::Numeric),
        PortDefinition::input(FMOD_DIVISOR_INPUT_PORT, "Divisor", PortDataType::Numeric),
        PortDefinition::output(
            NUMBER_RESULT_OUTPUT_PORT,
            "Result",
            PortDataType::Numeric,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
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
///
/// Persisted content and property maps are read-only through the public
/// authoring API. Serde can still load incomplete pre-v1 data losslessly.
///
/// ```compile_fail
/// use library::model::{GeneratorContent, Node, NodeContent};
/// let mut node = Node::new_merge("cannot reclassify");
/// node.content = NodeContent::Generator(GeneratorContent::Text);
/// ```
///
/// ```compile_fail
/// use library::model::property::PropertyMap;
/// use library::model::Node;
/// let mut node = Node::new_merge("cannot clear initialization");
/// node.properties = PropertyMap::new();
/// ```
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: Uuid,
    pub name: String,
    content: NodeContent,
    /// Authoritative authored evaluation state. Disabled Nodes produce
    /// NoOutput before resolving descriptors, properties, or upstream values.
    pub enabled: bool,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default)]
    properties: PropertyMap,
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

    /// Completion point for converter-backed Media Nodes. Definitions are
    /// validated and materialized here so no caller can inject or omit a raw
    /// property map after selecting the Media content variant.
    pub(crate) fn from_media_converter(
        name: &str,
        content: MediaContent,
        definitions: &[PropertyDefinition],
        file_path: String,
    ) -> Result<Self, String> {
        let mut properties = Self::default_properties("Media converter", definitions, true)?;
        if properties.get("file_path").is_some() {
            return Err(
                "Media converter must not declare reserved property 'file_path'".to_string(),
            );
        }
        properties.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path)),
        );
        Ok(Self::with_properties(
            name,
            NodeContent::Media(content),
            properties,
        ))
    }

    /// Creates a composition/reference source.
    pub fn new_reference(name: &str, content: ReferenceContent) -> Self {
        Self::with_properties(name, NodeContent::Reference(content), PropertyMap::new())
    }

    /// Completion point for descriptor-backed Plugin operations. The property
    /// map is always derived from the validated definitions; neither external
    /// nor internal callers can pair arbitrary content with a detached map.
    pub(crate) fn from_operation_parts(
        parts: crate::plugin::OperationNodeParts,
    ) -> Result<Self, String> {
        let (name, content, definitions) = parts.into_node_data();
        let properties = Self::default_properties("Plugin operation", &definitions, true)?;
        Ok(Self::with_properties(
            &name,
            NodeContent::PluginOperation(content),
            properties,
        ))
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

    fn default_properties(
        source: &str,
        definitions: &[PropertyDefinition],
        allow_empty: bool,
    ) -> Result<PropertyMap, String> {
        if definitions.is_empty() && !allow_empty {
            return Err(format!("{source} declared no properties"));
        }
        let mut names = std::collections::HashSet::with_capacity(definitions.len());
        for definition in definitions {
            definition.validate_definition().map_err(|error| {
                format!(
                    "{source} property '{}' has invalid metadata: {error}",
                    definition.name()
                )
            })?;
            if !names.insert(definition.name()) {
                return Err(format!(
                    "{source} declared duplicate property '{}'",
                    definition.name()
                ));
            }
        }
        Ok(PropertyMap::from_definitions(definitions))
    }

    /// Persisted execution kind. Content is immutable through the public
    /// authoring API; use the typed factories to create a different kind.
    pub fn content(&self) -> &NodeContent {
        &self.content
    }

    /// Authoritative authored values. The map is read-only as a collection so
    /// a complete factory result cannot be cleared or replaced accidentally.
    pub fn properties(&self) -> &PropertyMap {
        &self.properties
    }

    /// Replaces one factory-declared authored property. Unknown keys are not
    /// inserted: adding a key requires a definition-backed factory or an
    /// explicit persisted Serde payload.
    pub fn set_property(&mut self, key: String, property: Property) -> Result<(), String> {
        if self.properties.get(&key).is_none() {
            return Err(format!(
                "Node '{}' has no initialized property '{key}'",
                self.name
            ));
        }
        self.properties.set(key, property);
        Ok(())
    }

    /// Creates a generic native floating-point remainder Node.
    ///
    /// `x` is deliberately not implicit: a timeline loop is authored by
    /// wiring a container's Time output to `x`. `divisor` remains a normal,
    /// wire-overridable numeric property initialized to `1.0`.
    pub fn new_fmod(name: &str) -> Self {
        let content = ValueContent::Fmod;
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
        if self.properties.get(property_key).is_none() {
            return false;
        }
        self.properties
            .update_property_or_keyframe(property_key, time, value, easing);
        true
    }

    pub(crate) fn upsert_keyframe_with_id(
        &mut self,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Option<crate::model::property::KeyframeId> {
        self.properties.get(property_key)?;
        self.properties
            .upsert_keyframe_with_id(property_key, time, value, easing)
    }

    pub(crate) fn update_keyframe_by_id(
        &mut self,
        property_key: &str,
        keyframe_id: crate::model::property::KeyframeId,
        update: crate::model::property::KeyframeUpdate,
    ) -> bool {
        self.properties
            .get_mut(property_key)
            .is_some_and(|property| property.update_keyframe_by_id(keyframe_id, update))
    }

    pub(crate) fn remove_keyframe_by_id(
        &mut self,
        property_key: &str,
        keyframe_id: crate::model::property::KeyframeId,
    ) -> bool {
        self.properties
            .get_mut(property_key)
            .is_some_and(|property| property.remove_keyframe_by_id(keyframe_id))
    }

    pub(crate) fn set_property_attribute(
        &mut self,
        property_key: &str,
        attribute_key: String,
        attribute_value: PropertyValue,
    ) -> bool {
        let Some(property) = self.properties.get_mut(property_key) else {
            return false;
        };
        property.properties.insert(attribute_key, attribute_value);
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
    /// Native, typed numeric operations. Inputs and outputs remain canonical
    /// Project ports; this variant does not introduce a parallel value model.
    Value(ValueContent),
    /// Ordered variadic image compositor. Input ordering lives on canonical
    /// ProjectConnection::order, never on a UI pin index.
    Merge,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueContent {
    /// Generic component-wise floating-point remainder. The required `x`
    /// input and wire-overridable `divisor` accept scalar and 2D/3D/4D
    /// numeric values. Invalid inputs produce graph `NoOutput`; no Time input
    /// or timeline behavior is implicit.
    Fmod,
}

impl ValueContent {
    pub(crate) fn numeric_operation(self) -> NumericBinaryOperation {
        match self {
            Self::Fmod => NumericBinaryOperation::Fmod,
        }
    }

    pub fn primary_input(self) -> &'static str {
        match self {
            Self::Fmod => FMOD_X_INPUT_PORT,
        }
    }

    pub fn secondary_input(self) -> &'static str {
        match self {
            Self::Fmod => FMOD_DIVISOR_INPUT_PORT,
        }
    }

    /// Canonical authored-property metadata for this native numeric operation.
    /// Factories and inspectors consume this same definition list.
    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::Fmod => FMOD_PROPERTY_DEFINITIONS.as_slice(),
        }
    }

    /// Canonical graph ports for this native numeric operation.
    pub fn port_definitions(self) -> &'static [PortDefinition] {
        match self {
            Self::Fmod => FMOD_PORT_DEFINITIONS.as_slice(),
        }
    }

    /// Declares the primary input that a future bypass state should route to
    /// each output. This is operation metadata only; it deliberately does not
    /// add or reinterpret authored Node state.
    pub fn bypass_input_for_output(self, output: &str) -> Option<&'static str> {
        if output == NUMBER_RESULT_OUTPUT_PORT {
            Some(self.primary_input())
        } else {
            None
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
        let mut sparse = serde_json::to_value(Node::new_merge("persisted sparse generator"))?;
        sparse["content"] = serde_json::json!({ "type": "Generator", "data": "Text" });
        sparse["properties"] = serde_json::json!({});
        let json = serde_json::to_string(&sparse)?;
        let loaded: Node = serde_json::from_str(&json)?;

        assert_eq!(
            loaded.content(),
            &NodeContent::Generator(GeneratorContent::Text)
        );
        assert!(loaded.properties().iter().next().is_none());
        Ok(())
    }

    #[test]
    fn pre_v1_time_modulo_json_has_no_fmod_alias() -> Result<(), serde_json::Error> {
        let mut legacy = serde_json::to_value(Node::new_fmod("legacy value kind"))?;
        legacy["content"]["data"] = serde_json::Value::String("TimeModulo".to_string());
        let error = serde_json::from_value::<Node>(legacy).unwrap_err();
        assert!(error.to_string().contains("unknown variant `TimeModulo`"));
        Ok(())
    }

    #[test]
    fn authored_edits_cannot_extend_a_factory_property_contract() {
        let mut node = Node::new_fmod("sealed property contract");
        let unknown = Property::constant(PropertyValue::Number(OrderedFloat(2.0)));

        assert!(node.set_property("unknown".to_string(), unknown).is_err());
        assert!(!node.update_property_or_keyframe(
            "unknown",
            0.0,
            PropertyValue::Number(OrderedFloat(2.0)),
            None,
        ));
        assert!(
            node.upsert_keyframe_with_id(
                "unknown",
                0.0,
                PropertyValue::Number(OrderedFloat(2.0)),
                None,
            )
            .is_none()
        );
        assert!(node.properties().get("unknown").is_none());
        assert!(node.properties().get(FMOD_DIVISOR_INPUT_PORT).is_some());
        assert_eq!(
            ValueContent::Fmod.bypass_input_for_output(NUMBER_RESULT_OUTPUT_PORT),
            Some(FMOD_X_INPUT_PORT)
        );
    }
}
