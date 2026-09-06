//! Sample uniform leaves once per invocation; Point arithmetic runs on the GPU.

use super::*;
use crate::core::render_plan::{CompiledPointInstruction, CompiledPointProgram};
use crate::model::point::{PointAttributeElementType, PointInstruction, PointRenderProgram};
use crate::model::property::ColorValue;

impl ModuleImageRuntime<'_> {
    pub(super) fn sample_point_program(
        &mut self,
        compiled: &CompiledPointProgram,
    ) -> Result<PointRenderProgram, LibraryError> {
        let mut ramps = Vec::new();
        let mut ramp_addresses = HashMap::new();
        let instructions = compiled
            .instructions
            .iter()
            .map(|instruction| {
                Ok(match instruction {
                    CompiledPointInstruction::Uniform {
                        node_id,
                        port,
                        element_type,
                    } => {
                        let value = self.value_input(*node_id, port)?.ok_or_else(|| {
                            LibraryError::Validation(format!(
                                "Point uniform {node_id}:{port} produced no value"
                            ))
                        })?;
                        PointInstruction::Constant {
                            value: canonical_uniform(*element_type, value)?,
                        }
                    }
                    CompiledPointInstruction::Age => PointInstruction::Age,
                    CompiledPointInstruction::NormalizedAge => PointInstruction::NormalizedAge,
                    CompiledPointInstruction::Random { channel } => {
                        PointInstruction::Random { channel: *channel }
                    }
                    CompiledPointInstruction::LoadAttribute { attribute } => {
                        PointInstruction::LoadAttribute {
                            attribute: *attribute,
                        }
                    }
                    CompiledPointInstruction::StoreNumber { attribute, value } => {
                        PointInstruction::StoreNumber {
                            attribute: *attribute,
                            value: *value,
                        }
                    }
                    CompiledPointInstruction::Binary {
                        operation,
                        left,
                        right,
                    } => PointInstruction::Binary {
                        operation: *operation,
                        left: *left,
                        right: *right,
                    },
                    CompiledPointInstruction::ColorRamp { gradient, factor } => {
                        let index = if let Some(index) = ramp_addresses.get(gradient) {
                            *index
                        } else {
                            let Some(PropertyValue::Gradient(value)) =
                                self.value_input(gradient.node_id, &gradient.port)?
                            else {
                                return Err(LibraryError::Validation(
                                    "Point Color Ramp requires a Gradient value".into(),
                                ));
                            };
                            let index = u16::try_from(ramps.len()).map_err(|_| {
                                LibraryError::Validation(
                                    "Point Gradient register limit exceeded".into(),
                                )
                            })?;
                            ramps.push(value);
                            ramp_addresses.insert(gradient.clone(), index);
                            index
                        };
                        PointInstruction::ColorRamp {
                            gradient: index,
                            factor: *factor,
                        }
                    }
                })
            })
            .collect::<Result<Vec<_>, LibraryError>>()?;
        let program = PointRenderProgram {
            schema: compiled.schema.clone(),
            instructions,
            ramps,
            color_register: compiled.color_register,
        };
        program.validate().map_err(LibraryError::Validation)?;
        Ok(program)
    }
}

fn canonical_uniform(
    kind: PointAttributeElementType,
    value: PropertyValue,
) -> Result<PropertyValue, LibraryError> {
    let value = match (kind, value) {
        (PointAttributeElementType::Number, PropertyValue::Integer(value)) => {
            PropertyValue::Number(OrderedFloat(value as f64))
        }
        (PointAttributeElementType::Color, PropertyValue::Color(value)) => {
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&value))
        }
        (_, value) => value,
    };
    kind.pack_value(&value).map_err(LibraryError::Validation)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_conversion_uses_canonical_values_and_rejects_wrong_types() {
        assert_eq!(
            canonical_uniform(
                PointAttributeElementType::Number,
                PropertyValue::Integer(42)
            )
            .unwrap(),
            PropertyValue::Number(OrderedFloat(42.0))
        );
        let color = crate::model::frame::color::Color::white();
        assert_eq!(
            canonical_uniform(
                PointAttributeElementType::Color,
                PropertyValue::Color(color.clone())
            )
            .unwrap(),
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color))
        );
        assert!(
            canonical_uniform(
                PointAttributeElementType::Number,
                PropertyValue::String("42".into())
            )
            .is_err()
        );
        assert!(
            canonical_uniform(
                PointAttributeElementType::Number,
                PropertyValue::Number(OrderedFloat(f64::MAX))
            )
            .is_err()
        );
    }
}
