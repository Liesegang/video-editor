//! Connection model for the data-flow graph.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::project::property::PropertyValue;

/// Data type for a pin (Blender-style socket type).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PinDataType {
    /// Image/texture data flow
    Image,
    /// Floating point scalar (f64)
    Scalar,
    /// Integer value (i64)
    Integer,
    /// Boolean value
    Boolean,
    /// 2D vector
    Vec2,
    /// 3D vector
    Vec3,
    /// RGBA color
    Color,
    /// Text string
    String,
    /// SVG path data
    Path,
    /// Enumeration selection
    Enum,
    /// Style output (fill/stroke)
    Style,
    /// Video resource
    Video,
    /// Font reference
    Font,
    /// Blend mode selection
    BlendMode,
    /// Color gradient (color ramp)
    Gradient,
    /// 1D value curve (profile/timeline)
    Curve,
    /// Particle system data
    ParticleSystem,
    /// 3D camera
    Camera3D,
    /// 3D object
    Object3D,
    /// 3D material
    Material,
    /// Shape data (text glyphs, SVG path, etc.)
    Shape,
    /// Generic vector (N-dimensional)
    Vector,
    /// List/array of values
    List,
    /// Accepts any type (generic)
    Any,
}

/// Direction of a pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinDirection {
    Input,
    Output,
}

/// Definition of a pin on a node type.
#[derive(Clone, Debug)]
pub struct PinDefinition {
    /// Internal name used for connections (e.g. "image_in", "amount")
    pub name: String,
    /// Display name shown in the UI (e.g. "Image", "Amount")
    pub display_name: String,
    /// Whether this is an input or output pin
    pub direction: PinDirection,
    /// Data type of this pin
    pub data_type: PinDataType,
    /// Default value when no connection is present (for input pins)
    pub default_value: Option<PropertyValue>,
}

impl PinDefinition {
    pub fn input(name: &str, display_name: &str, data_type: PinDataType) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            direction: PinDirection::Input,
            data_type,
            default_value: None,
        }
    }

    pub fn output(name: &str, display_name: &str, data_type: PinDataType) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            direction: PinDirection::Output,
            data_type,
            default_value: None,
        }
    }

    pub fn with_default(mut self, value: PropertyValue) -> Self {
        self.default_value = Some(value);
        self
    }
}

/// Identifies a specific pin on a specific node.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PinId {
    pub node_id: Uuid,
    pub pin_name: String,
}

impl PinId {
    pub fn new(node_id: Uuid, pin_name: &str) -> Self {
        Self {
            node_id,
            pin_name: pin_name.to_string(),
        }
    }
}

/// A connection between two pins (an edge in the data-flow graph).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Connection {
    pub id: Uuid,
    /// Source pin (output)
    pub from: PinId,
    /// Destination pin (input)
    pub to: PinId,
}

impl Connection {
    pub fn new(from: PinId, to: PinId) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
        }
    }
}
