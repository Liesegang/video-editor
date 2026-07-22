use crate::model::blend::BlendMode;
use crate::model::numeric::NumericBinaryOperation;
use crate::model::project::connection::{
    DATA_VALUE_PROPERTY, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortDataType, PortDefinition, PortDirection,
    PortExposure, PortMultiplicity, PortSide,
};
use crate::model::project::property::{
    Property, PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue,
};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use uuid::Uuid;

mod catalog;
mod color;
mod containers;
mod data;
mod list;
mod path;
mod sound_analysis;
pub use catalog::{
    NativeNodeCatalogDescriptor, NativeNodeFactory, NativeNodeRuntimeStatus, native_node_catalog,
    native_node_descriptor, native_node_descriptor_for_node,
};
pub use color::{
    COLOR_ALPHA_PORT, COLOR_BLUE_PORT, COLOR_GREEN_PORT, COLOR_MIX_FACTOR_PORT,
    COLOR_MIX_LEFT_PORT, COLOR_MIX_RIGHT_PORT, COLOR_RED_PORT, COLOR_SPACE_PORT, COLOR_VALUE_PORT,
    ColorContent,
};
pub use containers::{
    CLIP_DURATION_PROPERTY, CLIP_START_TIME_PROPERTY, CLIP_TIME_STRETCH_PROPERTY,
    CLIP_TRIM_IN_PROPERTY, Clip, Track,
};
pub use data::DataContent;
pub use list::ListContent;
pub use path::PathOperationContent;
pub use sound_analysis::SoundAnalysisContent;

/// Stable authored/catalog identity of the native ordered Sound mixer.
pub const SOUND_MERGE_OPERATION_KEY: &str = "sound_merge";

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

static ADD_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> =
    LazyLock::new(|| [numeric_b_property_definition(0.0)]);
static SUBTRACT_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> =
    LazyLock::new(|| [numeric_b_property_definition(0.0)]);
static MULTIPLY_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> =
    LazyLock::new(|| [numeric_b_property_definition(1.0)]);
static DIVIDE_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> =
    LazyLock::new(|| [numeric_b_property_definition(1.0)]);

fn numeric_b_property_definition(default: f64) -> PropertyDefinition {
    PropertyDefinition::new(
        NUMERIC_B_INPUT_PORT,
        PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            step: 0.01,
            suffix: String::new(),
            min_hard_limit: false,
            max_hard_limit: false,
        },
        "B",
        PropertyValue::Number(OrderedFloat(default)),
    )
}

static BASIC_NUMERIC_PORT_DEFINITIONS: LazyLock<[PortDefinition; 3]> = LazyLock::new(|| {
    [
        PortDefinition::input(NUMERIC_A_INPUT_PORT, "A", PortDataType::Numeric),
        PortDefinition::input(NUMERIC_B_INPUT_PORT, "B", PortDataType::Numeric),
        PortDefinition::output(
            NUMBER_RESULT_OUTPUT_PORT,
            "Result",
            PortDataType::Numeric,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

fn fmod_property_definitions() -> &'static [PropertyDefinition] {
    FMOD_PROPERTY_DEFINITIONS.as_slice()
}

fn add_property_definitions() -> &'static [PropertyDefinition] {
    ADD_PROPERTY_DEFINITIONS.as_slice()
}

fn subtract_property_definitions() -> &'static [PropertyDefinition] {
    SUBTRACT_PROPERTY_DEFINITIONS.as_slice()
}

fn multiply_property_definitions() -> &'static [PropertyDefinition] {
    MULTIPLY_PROPERTY_DEFINITIONS.as_slice()
}

fn divide_property_definitions() -> &'static [PropertyDefinition] {
    DIVIDE_PROPERTY_DEFINITIONS.as_slice()
}

fn fmod_port_definitions() -> &'static [PortDefinition] {
    FMOD_PORT_DEFINITIONS.as_slice()
}

fn basic_numeric_port_definitions() -> &'static [PortDefinition] {
    BASIC_NUMERIC_PORT_DEFINITIONS.as_slice()
}

struct ValueOperationDescriptor {
    operation_key: &'static str,
    label: &'static str,
    symbol: &'static str,
    operation: NumericBinaryOperation,
    primary_input: &'static str,
    secondary_input: &'static str,
    result_output: &'static str,
    property_definitions: fn() -> &'static [PropertyDefinition],
    port_definitions: fn() -> &'static [PortDefinition],
}

static FMOD_VALUE_DESCRIPTOR: ValueOperationDescriptor = ValueOperationDescriptor {
    operation_key: "fmod",
    label: "Fmod",
    symbol: "%",
    operation: NumericBinaryOperation::Fmod,
    primary_input: FMOD_X_INPUT_PORT,
    secondary_input: FMOD_DIVISOR_INPUT_PORT,
    result_output: NUMBER_RESULT_OUTPUT_PORT,
    property_definitions: fmod_property_definitions,
    port_definitions: fmod_port_definitions,
};

static ADD_VALUE_DESCRIPTOR: ValueOperationDescriptor = ValueOperationDescriptor {
    operation_key: "add",
    label: "Add",
    symbol: "+",
    operation: NumericBinaryOperation::Add,
    primary_input: NUMERIC_A_INPUT_PORT,
    secondary_input: NUMERIC_B_INPUT_PORT,
    result_output: NUMBER_RESULT_OUTPUT_PORT,
    property_definitions: add_property_definitions,
    port_definitions: basic_numeric_port_definitions,
};

static SUBTRACT_VALUE_DESCRIPTOR: ValueOperationDescriptor = ValueOperationDescriptor {
    operation_key: "subtract",
    label: "Subtract",
    symbol: "−",
    operation: NumericBinaryOperation::Subtract,
    primary_input: NUMERIC_A_INPUT_PORT,
    secondary_input: NUMERIC_B_INPUT_PORT,
    result_output: NUMBER_RESULT_OUTPUT_PORT,
    property_definitions: subtract_property_definitions,
    port_definitions: basic_numeric_port_definitions,
};

static MULTIPLY_VALUE_DESCRIPTOR: ValueOperationDescriptor = ValueOperationDescriptor {
    operation_key: "multiply",
    label: "Multiply",
    symbol: "×",
    operation: NumericBinaryOperation::Multiply,
    primary_input: NUMERIC_A_INPUT_PORT,
    secondary_input: NUMERIC_B_INPUT_PORT,
    result_output: NUMBER_RESULT_OUTPUT_PORT,
    property_definitions: multiply_property_definitions,
    port_definitions: basic_numeric_port_definitions,
};

static DIVIDE_VALUE_DESCRIPTOR: ValueOperationDescriptor = ValueOperationDescriptor {
    operation_key: "divide",
    label: "Divide",
    symbol: "÷",
    operation: NumericBinaryOperation::Divide,
    primary_input: NUMERIC_A_INPUT_PORT,
    secondary_input: NUMERIC_B_INPUT_PORT,
    result_output: NUMBER_RESULT_OUTPUT_PORT,
    property_definitions: divide_property_definitions,
    port_definitions: basic_numeric_port_definitions,
};

/// A leaf graph node. It owns media/generator/composition-instance behavior and render
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
    /// Authoritative pass-through state, distinct from `enabled`. A bypassed
    /// Node routes a compatible single input to its same-typed output without
    /// evaluating the Node's descriptor or properties.
    #[serde(default)]
    pub bypassed: bool,
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

    /// Creates an ordered variadic Sound mixer. The input order is stored on
    /// canonical Project connections, not in a parallel mixer model.
    pub fn new_sound_merge(name: &str) -> Self {
        Self::with_properties(name, NodeContent::SoundMerge, PropertyMap::new())
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

    /// Creates a placement of one top-level Composition definition.
    pub fn new_composition_instance(name: &str, content: CompositionInstanceContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::CompositionInstance(content),
            PropertyMap::new(),
        )
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
            bypassed: false,
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

    /// Resolve the canonical pass-through input for one output. Operations
    /// participate only when one single input has the output's exact type;
    /// ambiguous multi-input operations require explicit operation metadata
    /// before they can be bypassed safely.
    pub fn bypass_input_for_output(&self, output: &str) -> Option<&str> {
        match self.content() {
            NodeContent::Value(value) => value.bypass_input_for_output(output),
            NodeContent::PluginOperation(operation) => {
                let output_type = operation
                    .declared_ports
                    .iter()
                    .find(|port| port.key == output && port.direction == PortDirection::Output)?
                    .data_type;
                let matching = operation
                    .declared_ports
                    .iter()
                    .filter(|port| {
                        port.direction == PortDirection::Input
                            && port.multiplicity == PortMultiplicity::Single
                            && port.data_type == output_type
                    })
                    .collect::<Vec<_>>();
                let [input] = matching.as_slice() else {
                    return None;
                };
                Some(input.key.as_str())
            }
            _ => None,
        }
    }

    pub fn supports_bypass(&self) -> bool {
        if matches!(self.content(), NodeContent::SoundMerge) {
            return true;
        }
        let ports = match self.content() {
            NodeContent::Value(value) => value.port_definitions(),
            NodeContent::PluginOperation(operation) => operation.declared_ports.as_slice(),
            _ => return false,
        };
        let outputs = ports
            .iter()
            .filter(|port| port.direction == PortDirection::Output)
            .collect::<Vec<_>>();
        !outputs.is_empty()
            && outputs.iter().all(|port| {
                matches!(
                    port.data_type,
                    PortDataType::Audio
                        | PortDataType::List
                        | PortDataType::Image
                        | PortDataType::Shape
                        | PortDataType::Numeric
                        | PortDataType::Number
                        | PortDataType::Integer
                        | PortDataType::Boolean
                        | PortDataType::String
                        | PortDataType::Color
                        | PortDataType::Path
                        | PortDataType::Vec2
                        | PortDataType::Vec3
                        | PortDataType::Vec4
                ) && self.bypass_input_for_output(&port.key).is_some()
            })
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
        if matches!(self.content(), NodeContent::Data(_) | NodeContent::Color(_)) {
            let value = property.value().ok_or_else(|| {
                format!(
                    "Typed metadata Node '{}' property '{key}' has no authored value",
                    self.name
                )
            })?;
            if !self.accepts_authored_property_value(&key, value) {
                return Err(format!(
                    "Typed metadata Node '{}' rejects an incompatible '{key}' value",
                    self.name,
                ));
            }
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
        Self::new_value(name, ValueContent::Fmod)
    }

    pub fn new_add(name: &str) -> Self {
        Self::new_value(name, ValueContent::Add)
    }

    pub fn new_subtract(name: &str) -> Self {
        Self::new_value(name, ValueContent::Subtract)
    }

    pub fn new_multiply(name: &str) -> Self {
        Self::new_value(name, ValueContent::Multiply)
    }

    pub fn new_divide(name: &str) -> Self {
        Self::new_value(name, ValueContent::Divide)
    }

    /// Creates a first-party heterogeneous List operation with every authored
    /// property initialized from its canonical metadata.
    pub fn new_list(name: &str, content: ListContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::List(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }

    /// Creates a lossless, color-space-tagged metadata operation.
    pub fn new_color(name: &str, content: ColorContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::Color(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }

    /// Creates a canonical authored data leaf with its complete typed default.
    pub fn new_data(name: &str, content: DataContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::Data(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }

    /// Creates an executable operation over canonical Path graph values.
    pub fn new_path_operation(name: &str, content: PathOperationContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::Path(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }

    /// Creates one of the native descriptor-backed numeric operations.
    pub fn new_value(name: &str, content: ValueContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::Value(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }

    /// Creates a detached native Node from its stable catalog identity.
    /// Canvas-backed Generators deliberately remain ProjectManager factories.
    pub fn new_catalog_node(catalog_id: &str) -> Result<Self, String> {
        let descriptor = native_node_descriptor(catalog_id)
            .ok_or_else(|| format!("Unknown native Node catalog id '{catalog_id}'"))?;
        descriptor.create_detached_node()
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
        if !self.accepts_authored_property_value(property_key, &value) {
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
        if !self.accepts_authored_property_value(property_key, &value) {
            return None;
        }
        self.properties
            .upsert_keyframe_with_id(property_key, time, value, easing)
    }

    pub(crate) fn update_keyframe_by_id(
        &mut self,
        property_key: &str,
        keyframe_id: crate::model::property::KeyframeId,
        update: crate::model::property::KeyframeUpdate,
    ) -> bool {
        if let Some(value) = update.value.as_ref()
            && !self.accepts_authored_property_value(property_key, value)
        {
            return false;
        }
        self.properties
            .get_mut(property_key)
            .is_some_and(|property| property.update_keyframe_by_id(keyframe_id, update))
    }

    fn accepts_authored_property_value(&self, key: &str, value: &PropertyValue) -> bool {
        match self.content() {
            NodeContent::Data(data) => key == DATA_VALUE_PROPERTY && data.accepts_value(value),
            NodeContent::Color(operation) => operation.accepts_property(key, value),
            _ => true,
        }
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
    CompositionInstance(CompositionInstanceContent),
    /// A plugin-defined graph operation whose authored state is entirely
    /// represented by this stable identity, its persisted port contract, and
    /// [`Node::properties`]. Loading and validating a Project never requires
    /// the referenced plugin to be installed.
    PluginOperation(PluginOperationContent),
    /// Native, typed numeric operations. Inputs and outputs remain canonical
    /// Project ports; this variant does not introduce a parallel value model.
    Value(ValueContent),
    /// First-party heterogeneous List operations. Values are evaluated as
    /// serializable `PropertyValue::Array` payloads; connection order remains
    /// authoritative on `ProjectConnection::order`.
    List(ListContent),
    /// Lossless straight-alpha floating-point Color operations. These stay in
    /// the metadata graph and do not cross the current RGBA8 image boundary.
    Color(ColorContent),
    /// First-party authored Color and Path leaves. Their values live only in
    /// the canonical Project property map and retain their tagged precision.
    Data(DataContent),
    /// Executable first-party operations over canonical Path graph values.
    /// These return reusable Project data rather than annotating a transient
    /// render Shape like a Shape Path Effect.
    Path(PathOperationContent),
    /// A first-party typed operation whose authoring and port contract are
    /// available, while its runtime may still be explicitly design-needed.
    NativeOperation(NativeOperationContent),
    /// Ordered variadic image compositor. Input ordering lives on canonical
    /// ProjectConnection::order, never on a UI pin index.
    Merge,
    /// Ordered variadic Sound mixer. Runtime audio routing traverses these
    /// typed connections before the sample mixer combines Media leaves.
    SoundMerge,
    /// Native frame-time Sound analysis. PCM and Spectrum values are
    /// transient evaluation data and never become a persisted side model.
    SoundAnalysis(SoundAnalysisContent),
}

impl NodeContent {
    pub fn is_semantic_visual_source(&self) -> bool {
        matches!(
            self,
            Self::Media(_) | Self::Generator(_) | Self::CompositionInstance(_)
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueContent {
    /// Generic component-wise floating-point remainder. The required `x`
    /// input and wire-overridable `divisor` accept scalar and 2D/3D/4D
    /// numeric values. Invalid inputs produce graph `NoOutput`; no Time input
    /// or timeline behavior is implicit.
    Fmod,
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl ValueContent {
    pub const ALL: [Self; 5] = [
        Self::Fmod,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
    ];

    fn descriptor(self) -> &'static ValueOperationDescriptor {
        match self {
            Self::Fmod => &FMOD_VALUE_DESCRIPTOR,
            Self::Add => &ADD_VALUE_DESCRIPTOR,
            Self::Subtract => &SUBTRACT_VALUE_DESCRIPTOR,
            Self::Multiply => &MULTIPLY_VALUE_DESCRIPTOR,
            Self::Divide => &DIVIDE_VALUE_DESCRIPTOR,
        }
    }

    /// Stable semantic identity used by menus, diagnostics, and automation.
    pub fn operation_key(self) -> &'static str {
        self.descriptor().operation_key
    }

    pub fn label(self) -> &'static str {
        self.descriptor().label
    }

    pub fn symbol(self) -> &'static str {
        self.descriptor().symbol
    }

    pub(crate) fn numeric_operation(self) -> NumericBinaryOperation {
        self.descriptor().operation
    }

    pub fn primary_input(self) -> &'static str {
        self.descriptor().primary_input
    }

    pub fn secondary_input(self) -> &'static str {
        self.descriptor().secondary_input
    }

    /// Canonical authored-property metadata for this native numeric operation.
    /// Factories and inspectors consume this same definition list.
    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        (self.descriptor().property_definitions)()
    }

    /// Canonical graph ports for this native numeric operation.
    pub fn port_definitions(self) -> &'static [PortDefinition] {
        (self.descriptor().port_definitions)()
    }

    /// Declares the primary input that bypass routes to the result output.
    pub fn bypass_input_for_output(self, output: &str) -> Option<&'static str> {
        let descriptor = self.descriptor();
        if output == descriptor.result_output {
            Some(descriptor.primary_input)
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

/// Persisted stable identity for a first-party catalog operation. Its typed
/// ports and runtime status come from the central native catalog.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct NativeOperationContent {
    pub catalog_id: String,
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

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeneratorContent {
    Shape,
    Text,
    Solid,
    SkSL,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CompositionInstanceContent {
    /// Stable identity of the top-level Composition definition evaluated by
    /// this placement. Timing remains owned by the containing Clip, while
    /// spatial placement belongs to a downstream Image Transform operation;
    /// the referenced definition is never nested or reparented.
    pub composition_id: Uuid,
}

#[cfg(test)]
mod tests;
