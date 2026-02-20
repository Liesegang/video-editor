//! Node type definitions for the data-flow graph.

use crate::model::project::connection::PinDefinition;
use crate::model::project::property::PropertyDefinition;

/// Category of a node type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeCategory {
    /// Image effects (blur, dilate, drop_shadow, etc.)
    Effect,
    /// Drawing styles (fill, stroke)
    Style,
    /// Ensemble effectors (transform, step_delay, randomize, opacity)
    Effector,
    /// Ensemble decorators (backplate)
    Decorator,
    /// Mathematical operations (add, multiply, clamp, etc.)
    Math,
    /// Color operations (color_correct, hue_shift, etc.)
    Color,
    /// Generator nodes (noise, gradient, etc.)
    Generator,
    /// Logic/control flow (switch, compare, etc.)
    Logic,
    /// Data conversion (vec2_compose, vec2_decompose, etc.)
    Data,
    /// Scripting (expression, python)
    Scripting,
    /// Text operations (text layout, string manipulation)
    Text,
    /// Compositing (transform, blend, mask)
    Compositing,
    /// Filters (blur, glow, drop shadow)
    Filters,
    /// Distortion effects (displacement map)
    Distortion,
    /// Path operations (offset, fill, stroke, trim)
    Path,
    /// Time operations (time shift)
    Time,
    /// Image operations (channel split/combine, color space)
    Image,
    /// Particle system nodes
    Particles,
    /// 3D operations (camera, transform, render)
    ThreeD,
    /// Plugin-defined custom category
    Custom,
}

impl std::fmt::Display for NodeCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NodeCategory::Effect => "Effect",
            NodeCategory::Style => "Style",
            NodeCategory::Effector => "Effector",
            NodeCategory::Decorator => "Decorator",
            NodeCategory::Math => "Math",
            NodeCategory::Color => "Color",
            NodeCategory::Generator => "Generator",
            NodeCategory::Logic => "Logic",
            NodeCategory::Data => "Data",
            NodeCategory::Scripting => "Scripting",
            NodeCategory::Text => "Text",
            NodeCategory::Compositing => "Compositing",
            NodeCategory::Filters => "Filters",
            NodeCategory::Distortion => "Distortion",
            NodeCategory::Path => "Path",
            NodeCategory::Time => "Time",
            NodeCategory::Image => "Image",
            NodeCategory::Particles => "Particles",
            NodeCategory::ThreeD => "3D",
            NodeCategory::Custom => "Custom",
        };
        write!(f, "{}", s)
    }
}

/// Definition of a node type, registered in the PluginManager.
///
/// This describes what a node of this type looks like: its pins, default properties,
/// and metadata. Actual node instances are `GraphNode` structs whose `type_id`
/// references a `NodeTypeDefinition`.
#[derive(Debug, Clone)]
pub struct NodeTypeDefinition {
    /// Unique type identifier (e.g. "effect.blur", "style.fill", "math.add")
    pub type_id: String,
    /// Human-readable name (e.g. "Gaussian Blur")
    pub display_name: String,
    /// Category for grouping in the UI
    pub category: NodeCategory,
    /// Description shown in tooltips
    pub description: String,
    /// Input pin definitions
    pub inputs: Vec<PinDefinition>,
    /// Output pin definitions
    pub outputs: Vec<PinDefinition>,
    /// Default properties for new instances of this node type
    pub default_properties: Vec<PropertyDefinition>,
}

impl NodeTypeDefinition {
    pub fn new(type_id: &str, display_name: &str, category: NodeCategory) -> Self {
        Self {
            type_id: type_id.to_string(),
            display_name: display_name.to_string(),
            category,
            description: String::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            default_properties: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    pub fn with_inputs(mut self, inputs: Vec<PinDefinition>) -> Self {
        self.inputs = inputs;
        self
    }

    pub fn with_outputs(mut self, outputs: Vec<PinDefinition>) -> Self {
        self.outputs = outputs;
        self
    }

    pub fn with_properties(mut self, props: Vec<PropertyDefinition>) -> Self {
        self.default_properties = props;
        self
    }
}
