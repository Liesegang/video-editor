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
/// Ordered heterogeneous value input used by the native Make List Node.
pub const LIST_ITEMS_INPUT_PORT: &str = "item";
pub const LIST_INPUT_PORT: &str = "list";
pub const LIST_OUTPUT_PORT: &str = "list";
pub const LIST_INDEX_INPUT_PORT: &str = "index";
pub const LIST_ITEM_OUTPUT_PORT: &str = "item";
pub const LIST_LENGTH_OUTPUT_PORT: &str = "length";
pub const SOUND_INPUT_PORT: &str = "sound";
pub const SPECTRUM_INPUT_PORT: &str = "spectrum_in";
pub const SPECTRUM_OUTPUT_PORT: &str = "spectrum";
pub const ANALYSIS_WINDOW_MS_PROPERTY: &str = "window_ms";
pub const ANALYSIS_HOP_MS_PROPERTY: &str = "hop_ms";
pub const ANALYSIS_SAMPLE_RATE_PROPERTY: &str = "sample_rate";
pub const BAND_LOW_HZ_PROPERTY: &str = "low_hz";
pub const BAND_HIGH_HZ_PROPERTY: &str = "high_hz";

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
    /// A serializable, heterogeneous
    /// [`PropertyValue::Array`](crate::model::property::PropertyValue::Array)
    /// graph value.
    ///
    /// This is intentionally distinct from a variadic port. A variadic `Any`
    /// input receives several individual values; `List` transports one
    /// ordered array value through a single wire.
    List,
    Image,
    /// Render-time vector/typographic value. This is distinct from canonical
    /// authored `Path` geometry.
    Shape,
    Audio,
    /// Transient frequency-domain Sound value. Spectrum payloads are never
    /// persisted in Project; only this typed connection contract is authored.
    Spectrum,
    /// A scalar or 2D/3D/4D floating-point graph value. Integer sources are
    /// promoted to a scalar. Runtime values keep their concrete dimension.
    Numeric,
    Number,
    Integer,
    Boolean,
    String,
    Color,
    /// Canonical authored multi-contour path geometry.
    Path,
    Vec2,
    Vec3,
    Vec4,
    /// A catalog-defined option value. Concrete option sets belong to the
    /// descriptor rather than to the connection type.
    Enum,
    Asset,
    Gradient,
    Curve,
    ParticleSystem,
    Material,
    Geometry3D,
    Object3D,
    Object3DList,
    Camera3D,
    PointSource,
    Instance3D,
    Effector3D,
    EffectorStack,
    Field3D,
    FieldStack,
    /// Shared boundary for future Delay, Spring, and Elastic behaviors.
    MotionBehavior,
}

impl PortDataType {
    /// Return whether a connection is statically possible.
    ///
    /// `Any` is dynamic rather than an unchecked cast. Consumers must still
    /// validate the concrete `PropertyValue` at evaluation time and produce
    /// `NoOutput` when it does not match their required type.
    pub fn accepts(self, source: Self) -> bool {
        self == source
            || (self == Self::Any && source.is_property_value_family())
            || (source == Self::Any && self.is_property_value_family())
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

    /// Types whose runtime payload is represented by the serializable
    /// `PropertyValue` graph domain. Media, Shape, and transient analysis/3D
    /// handles are deliberately excluded from `Any`: accepting those would
    /// promise a heterogeneous payload representation that does not exist.
    pub const fn is_property_value_family(self) -> bool {
        matches!(
            self,
            Self::Any
                | Self::List
                | Self::Numeric
                | Self::Number
                | Self::Integer
                | Self::Boolean
                | Self::String
                | Self::Color
                | Self::Path
                | Self::Vec2
                | Self::Vec3
                | Self::Vec4
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
