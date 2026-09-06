//! Bounded, derived render-stage Point fields. No runtime arrays enter a Project.

use serde::{Deserialize, Serialize};

use super::{NumericBinaryOperation, PointAttributeElementType, PointAttributeSchema};
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
    Random {
        channel: u32,
    },
    LoadAttribute {
        attribute: u16,
    },
    StoreNumber {
        attribute: u16,
        value: u16,
    },
    Binary {
        operation: NumericBinaryOperation,
        left: u16,
        right: u16,
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
}

impl PointRenderProgram {
    pub fn validate(&self) -> Result<(), String> {
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
        // Number stores are the first supported operation. Other schema types
        // must not silently acquire numeric execution semantics.
        if self
            .schema
            .attributes()
            .iter()
            .any(|attribute| attribute.element_type() != PointAttributeElementType::Number)
        {
            return Err(
                "Point render-stage stores currently support Number attributes only".into(),
            );
        }
        let mut registers = Vec::with_capacity(self.instructions.len());
        let mut stored = vec![false; self.schema.attributes().len()];
        for instruction in &self.instructions {
            let kind = match instruction {
                PointInstruction::Constant { value } => {
                    let kind = match value {
                        PropertyValue::Number(_) => PointAttributeElementType::Number,
                        PropertyValue::ColorValue(_) => PointAttributeElementType::Color,
                        _ => {
                            return Err(
                                "Point constants must be canonical Number or Color values".into()
                            );
                        }
                    };
                    kind.pack_value(value)?;
                    kind
                }
                PointInstruction::Age | PointInstruction::NormalizedAge => {
                    PointAttributeElementType::Number
                }
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
                    PointAttributeElementType::Number
                }
                PointInstruction::StoreNumber { attribute, value } => {
                    require_register(&registers, *value, PointAttributeElementType::Number)?;
                    let target = stored
                        .get_mut(usize::from(*attribute))
                        .ok_or("Point Store references a missing attribute")?;
                    if *target {
                        return Err("Point attribute has multiple stores in one program".into());
                    }
                    *target = true;
                    PointAttributeElementType::Number
                }
                PointInstruction::Binary { left, right, .. } => {
                    require_register(&registers, *left, PointAttributeElementType::Number)?;
                    require_register(&registers, *right, PointAttributeElementType::Number)?;
                    PointAttributeElementType::Number
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
        )
    }
}

fn require_register(
    registers: &[PointAttributeElementType],
    index: u16,
    expected: PointAttributeElementType,
) -> Result<(), String> {
    match registers.get(usize::from(index)) {
        Some(actual) if *actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "Point register {index} requires {expected:?}, got {actual:?}"
        )),
        None => Err(format!(
            "Point register {index} must reference an earlier instruction"
        )),
    }
}
