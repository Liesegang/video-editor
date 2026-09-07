//! Bounded, derived render-stage Point fields. No runtime arrays enter a Project.

use serde::{Deserialize, Serialize};

use super::{NumericBinaryOperation, PointAttributeElementType, PointAttributeSchema};
use crate::model::ComparisonOperation;
use crate::model::numeric::NumericShape;
use crate::model::property::{GradientValue, PropertyValue};

pub const POINT_MAX_INSTRUCTIONS: usize = 64;
pub const POINT_MAX_RAMPS: usize = 8;
pub const POINT_MAX_RAMP_STOPS: usize = 64;

/// SSA register indices address earlier instructions, never arbitrary GPU memory.
/// Stores capture post-simulation values each rendered frame, not mutable history.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointInstruction {
    Constant {
        value: PropertyValue,
    },
    Age,
    NormalizedAge,
    /// Producer-local position after simulation, before the Sprite transform.
    Position,
    /// Producer-local point size before render-stage Point operations.
    Size,
    Random {
        channel: u32,
    },
    LoadAttribute {
        attribute: u16,
    },
    StoreAttribute {
        attribute: u16,
        value: u16,
    },
    Binary {
        operation: NumericBinaryOperation,
        left: u16,
        right: u16,
    },
    Length {
        value: u16,
    },
    Compare {
        operation: ComparisonOperation,
        left: u16,
        right: u16,
    },
    Select {
        element_type: PointAttributeElementType,
        condition: u16,
        when_true: u16,
        when_false: u16,
    },
    ColorRamp {
        gradient: u16,
        factor: u16,
    },
}

/// A sampled Frame command, not an authored or persisted Module topology.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointRenderProgram {
    pub schema: PointAttributeSchema,
    pub instructions: Vec<PointInstruction>,
    pub ramps: Vec<GradientValue>,
    pub color_register: u16,
    /// Final derived position for the rendered stream. `None` preserves the
    /// producer's position without allocating a parallel authored value.
    pub position_register: Option<u16>,
    /// Final derived size for the rendered stream. `None` preserves the
    /// producer's size.
    pub size_register: Option<u16>,
    /// Optional normalized image-choice field for the shared Sprite renderer.
    /// This is render-only and never changes simulation state.
    pub sprite_selection_register: Option<u16>,
}

impl PointRenderProgram {
    pub fn has_geometry_output(&self) -> bool {
        self.position_register.is_some() || self.size_register.is_some()
    }

    pub fn validate(&self) -> Result<(), String> {
        self.register_types().map(drop)
    }

    /// Validate the complete program and return the concrete type of every
    /// SSA register. GPU lowering consumes this result instead of maintaining
    /// a second numeric broadcast/type-inference implementation.
    pub(crate) fn register_types(&self) -> Result<Vec<PointAttributeElementType>, String> {
        self.schema.validate()?;
        if self.instructions.is_empty() || self.instructions.len() > POINT_MAX_INSTRUCTIONS {
            return Err(format!(
                "Point program requires 1..={POINT_MAX_INSTRUCTIONS} instructions"
            ));
        }
        if self.ramps.len() > POINT_MAX_RAMPS {
            return Err(format!(
                "Point program supports at most {POINT_MAX_RAMPS} ramps"
            ));
        }
        for ramp in &self.ramps {
            if ramp.stops().len() > POINT_MAX_RAMP_STOPS {
                return Err(format!(
                    "GPU Point ramps support at most {POINT_MAX_RAMP_STOPS} stops"
                ));
            }
            for stop in ramp.stops() {
                PointAttributeElementType::Color
                    .pack_value(&PropertyValue::ColorValue(stop.color().clone()))?;
            }
        }
        let mut registers = Vec::with_capacity(self.instructions.len());
        let mut stored = vec![false; self.schema.attributes().len()];
        for instruction in &self.instructions {
            let kind = match instruction {
                PointInstruction::Constant { value } => {
                    let kind = PointAttributeElementType::from_property_value(value)?;
                    kind.pack_value(value)?;
                    kind
                }
                PointInstruction::Age | PointInstruction::NormalizedAge => {
                    PointAttributeElementType::Number
                }
                PointInstruction::Position => PointAttributeElementType::Vec3,
                PointInstruction::Size => PointAttributeElementType::Number,
                PointInstruction::Random { channel } => {
                    if *channel >= 3 {
                        return Err("Point random channel must be in 0..3".into());
                    }
                    PointAttributeElementType::Number
                }
                PointInstruction::LoadAttribute { attribute } => {
                    if stored.get(usize::from(*attribute)) != Some(&true) {
                        return Err(
                            "Point attribute read requires an earlier Store in this stream".into(),
                        );
                    }
                    self.schema.attributes()[usize::from(*attribute)].element_type()
                }
                PointInstruction::StoreAttribute { attribute, value } => {
                    let kind = self
                        .schema
                        .attributes()
                        .get(usize::from(*attribute))
                        .ok_or("Point Store references a missing attribute")?
                        .element_type();
                    require_register(&registers, *value, kind)?;
                    let target = stored
                        .get_mut(usize::from(*attribute))
                        .ok_or("Point Store references a missing attribute")?;
                    if *target {
                        return Err("Point attribute has multiple stores in one program".into());
                    }
                    *target = true;
                    kind
                }
                PointInstruction::Binary { left, right, .. } => {
                    PointAttributeElementType::numeric_binary_result(
                        register_type(&registers, *left)?,
                        register_type(&registers, *right)?,
                    )?
                }
                PointInstruction::Length { value } => {
                    register_type(&registers, *value)?.numeric_shape()?;
                    PointAttributeElementType::Number
                }
                PointInstruction::Compare { left, right, .. } => {
                    require_register(&registers, *left, PointAttributeElementType::Number)?;
                    require_register(&registers, *right, PointAttributeElementType::Number)?;
                    PointAttributeElementType::Boolean
                }
                PointInstruction::Select {
                    element_type,
                    condition,
                    when_true,
                    when_false,
                } => {
                    require_register(&registers, *condition, PointAttributeElementType::Boolean)?;
                    require_register(&registers, *when_true, *element_type)?;
                    require_register(&registers, *when_false, *element_type)?;
                    *element_type
                }
                PointInstruction::ColorRamp { gradient, factor } => {
                    require_register(&registers, *factor, PointAttributeElementType::Number)?;
                    if usize::from(*gradient) >= self.ramps.len() {
                        return Err("Point Color Ramp references a missing Gradient".into());
                    }
                    PointAttributeElementType::Color
                }
            };
            registers.push(kind);
        }
        if stored.iter().any(|written| !written) {
            return Err("Point schema contains an attribute without a Store".into());
        }
        require_register(
            &registers,
            self.color_register,
            PointAttributeElementType::Color,
        )?;
        if let Some(position) = self.position_register {
            require_register(&registers, position, PointAttributeElementType::Vec3)?;
        }
        if let Some(size) = self.size_register {
            require_register(&registers, size, PointAttributeElementType::Number)?;
        }
        if let Some(selection) = self.sprite_selection_register {
            require_register(&registers, selection, PointAttributeElementType::Number)?;
        }
        Ok(registers)
    }
}

impl PointAttributeElementType {
    pub(crate) fn numeric_shape(self) -> Result<NumericShape, String> {
        match self {
            Self::Number => Ok(NumericShape::Scalar),
            Self::Vec2 => Ok(NumericShape::Vec2),
            Self::Vec3 => Ok(NumericShape::Vec3),
            Self::Vec4 => Ok(NumericShape::Vec4),
            Self::Integer | Self::Boolean | Self::Color => {
                Err(format!("Point numeric operation does not accept {self:?}"))
            }
        }
    }

    pub(crate) fn numeric_binary_result(self, other: Self) -> Result<Self, String> {
        let shape = self
            .numeric_shape()?
            .broadcast_result(other.numeric_shape()?)
            .map_err(|error| format!("Point numeric operands are incompatible: {error:?}"))?;
        Ok(match shape {
            NumericShape::Scalar => Self::Number,
            NumericShape::Vec2 => Self::Vec2,
            NumericShape::Vec3 => Self::Vec3,
            NumericShape::Vec4 => Self::Vec4,
        })
    }
}

fn register_type(
    registers: &[PointAttributeElementType],
    index: u16,
) -> Result<PointAttributeElementType, String> {
    registers
        .get(usize::from(index))
        .copied()
        .ok_or_else(|| format!("Point register {index} must reference an earlier instruction"))
}

fn require_register(
    registers: &[PointAttributeElementType],
    index: u16,
    expected: PointAttributeElementType,
) -> Result<(), String> {
    match register_type(registers, index) {
        Ok(actual) if actual == expected => Ok(()),
        Ok(actual) => Err(format!(
            "Point register {index} requires {expected:?}, got {actual:?}"
        )),
        Err(error) => Err(error),
    }
}
