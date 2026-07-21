use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::BlendMode;

pub const IMAGE_OUTPUT_PORT: &str = "image";
pub const AUDIO_OUTPUT_PORT: &str = "audio";
pub const IMAGE_INPUT_PORT: &str = "image_in";
pub const MERGE_IMAGES_PORT: &str = "images";
/// Ordered variadic Sound input used by native Sound Merge Nodes.
pub const MERGE_SOUNDS_PORT: &str = "sounds";
pub const SHAPE_OUTPUT_PORT: &str = "shape";
pub const SHAPE_INPUT_PORT: &str = "shape_in";
/// Separately addressed geometry template used by two-Shape operations such
/// as Backplate, distinct from their primary target Shape.
pub const BACKGROUND_SHAPE_INPUT_PORT: &str = "background_shape";
pub const TIME_PORT: &str = "time";
pub const FRAME_PORT: &str = "frame";
pub const FPS_PORT: &str = "fps";
pub const DURATION_PORT: &str = "duration";
pub const RESOLUTION_PORT: &str = "resolution";
pub const FMOD_X_INPUT_PORT: &str = "x";
pub const FMOD_DIVISOR_INPUT_PORT: &str = "divisor";
pub const NUMERIC_A_INPUT_PORT: &str = "a";
pub const NUMERIC_B_INPUT_PORT: &str = "b";
pub const NUMBER_RESULT_OUTPUT_PORT: &str = "result";

/// A graph result; `NoOutput` is distinct from every valid value in `T`.
#[derive(Clone, PartialEq, Debug)]
pub enum EvalOutput<T> {
    Produced(T),
    NoOutput,
}

impl<T> EvalOutput<T> {
    pub fn as_ref(&self) -> EvalOutput<&T> {
        match self {
            Self::Produced(value) => EvalOutput::Produced(value),
            Self::NoOutput => EvalOutput::NoOutput,
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> EvalOutput<U> {
        match self {
            Self::Produced(value) => EvalOutput::Produced(map(value)),
            Self::NoOutput => EvalOutput::NoOutput,
        }
    }
}

pub type EvaluationError = crate::error::LibraryError;
pub type EvalResult<T> = Result<EvalOutput<T>, EvaluationError>;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(tag = "owner_type", content = "owner_id")]
pub enum PortOwner {
    Composition(Uuid),
    Track(Uuid),
    Clip(Uuid),
    Node(Uuid),
}

impl PortOwner {
    pub fn id(self) -> Uuid {
        match self {
            Self::Composition(id) | Self::Track(id) | Self::Clip(id) | Self::Node(id) => id,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PortAddress {
    pub owner: PortOwner,
    pub port: String,
}

impl PortAddress {
    pub fn new(owner: PortOwner, port: impl Into<String>) -> Self {
        Self {
            owner,
            port: port.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortDirection {
    Input,
    Output,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortSide {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortExposure {
    Graph,
    Internal,
    External,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PortMultiplicity {
    #[default]
    Single,
    Variadic,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortDataType {
    Any,
    Image,
    /// Render-time vector/typographic value. This is distinct from `Path`,
    /// which is only an authored scalar SVG path string.
    Shape,
    Audio,
    /// A scalar or 2D/3D/4D floating-point graph value. Integer sources are
    /// promoted to a scalar. Runtime values keep their concrete dimension.
    Numeric,
    Number,
    Integer,
    Boolean,
    String,
    Color,
    Path,
    Vec2,
    Vec3,
    Vec4,
}

impl PortDataType {
    pub fn accepts(self, source: Self) -> bool {
        self == Self::Any
            || source == Self::Any
            || self == source
            || (self == Self::Number && source == Self::Integer)
            || ((self == Self::Numeric || source == Self::Numeric)
                && self.is_numeric_family()
                && source.is_numeric_family())
    }

    fn is_numeric_family(self) -> bool {
        matches!(
            self,
            Self::Numeric | Self::Number | Self::Integer | Self::Vec2 | Self::Vec3 | Self::Vec4
        )
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PortDefinition {
    pub key: String,
    pub label: String,
    pub direction: PortDirection,
    pub side: PortSide,
    pub exposure: PortExposure,
    pub data_type: PortDataType,
    pub multiplicity: PortMultiplicity,
}

impl PortDefinition {
    pub fn input(key: &str, label: &str, data_type: PortDataType) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            direction: PortDirection::Input,
            side: PortSide::Left,
            exposure: PortExposure::Graph,
            data_type,
            multiplicity: PortMultiplicity::Single,
        }
    }

    pub fn output(
        key: &str,
        label: &str,
        data_type: PortDataType,
        side: PortSide,
        exposure: PortExposure,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            direction: PortDirection::Output,
            side,
            exposure,
            data_type,
            multiplicity: PortMultiplicity::Single,
        }
    }

    pub fn variadic(mut self) -> Self {
        self.multiplicity = PortMultiplicity::Variadic;
        self
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProjectConnection {
    pub id: Uuid,
    pub from: PortAddress,
    pub to: PortAddress,
    /// Stable evaluation order for variadic inputs. It is independent of UI
    /// pin indices and remains meaningful after layout changes.
    pub order: i64,
    /// Compositing mode owned by this wire. This is meaningful only for an
    /// Image connection targeting a Merge Node's variadic `images` input.
    pub blend_mode: BlendMode,
}

impl ProjectConnection {
    pub fn new(from: PortAddress, to: PortAddress, order: i64) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            order,
            blend_mode: BlendMode::Normal,
        }
    }
}
