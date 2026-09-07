//! Sample uniform leaves once per invocation; Point arithmetic runs on the GPU.

use ordered_float::OrderedFloat;

use super::frame_values::{
    finite_f32, required_color, required_number, required_u32, required_vec3, transparent,
};
use super::*;
use crate::core::render_plan::{
    CompiledPointInstruction, CompiledPointProgram, CompiledPointRenderer, CompiledPointSource,
    CompiledPointValueType,
};
use crate::model::authoring::ModuleOutputId;
use crate::model::frame::point::{
    PointGridParameters, PointSceneFrame, PointSceneSource, SceneInvocationKey,
};
use crate::model::point::{PointAttributeElementType, PointInstruction, PointRenderProgram};
use crate::model::property::ColorValue;

impl ModuleImageRuntime<'_> {
    pub(super) fn evaluate_point_renderer(
        &mut self,
        output_id: ModuleOutputId,
        renderer: &CompiledPointRenderer,
    ) -> Result<FrameItem, LibraryError> {
        let renderer_node = self
            .definition
            .nodes
            .get(&renderer.renderer_node_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Compiled Point renderer reaches missing Node {}",
                    renderer.renderer_node_id
                ))
            })?;
        let point_program = renderer
            .point_program
            .as_ref()
            .map(|program| self.sample_point_program(program))
            .transpose()?;
        let color = if point_program.is_some() {
            // The Point program owns per-point color. Never send a varying
            // branch through the ordinary frame-uniform property evaluator.
            crate::model::frame::color::Color::white()
        } else {
            required_color(
                &self.node_values(&renderer_node)?,
                "color",
                "Sprite Renderer",
            )?
        };
        let (source_node_id, source) = match &renderer.source {
            CompiledPointSource::Particle(particle) => (
                particle.emitter_node_id,
                self.sample_particle_source(particle)?,
            ),
            CompiledPointSource::Grid { node_id } => (*node_id, self.sample_point_grid(*node_id)?),
        };
        let logical_width = u32::try_from(self.width).map_err(|_| {
            LibraryError::Validation("Point canvas width exceeds GPU limits".to_string())
        })?;
        let logical_height = u32::try_from(self.height).map_err(|_| {
            LibraryError::Validation("Point canvas height exceeds GPU limits".to_string())
        })?;
        let scene = PointSceneFrame {
            invocation: SceneInvocationKey {
                instance_path: self.instance_path.clone(),
                module_instance_id: self.invocation.instance_id,
                state_slot_id: renderer.state_slot_id,
                output_id,
            },
            source_node_id,
            executable_hash: self.definition.fingerprint,
            logical_width,
            logical_height,
            source,
            color,
            point_program,
        };
        scene.validate().map_err(LibraryError::Validation)?;
        let object = FrameItem::Object(FrameObject {
            source_node_id: renderer.renderer_node_id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(
                0.0,
                0.0,
                logical_width as f32,
                logical_height as f32,
            )),
            content: FrameContent::PointScene {
                scene,
                effects: Vec::new(),
                transform: Transform::default(),
            },
        });
        Ok(FrameItem::Group(FrameGroup {
            source_id: renderer_node.id,
            kind: FrameGroupKind::Node,
            width: self.width,
            height: self.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: renderer_node.blend_mode,
            effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
            effects: Vec::new(),
            items: vec![object],
        }))
    }

    fn sample_point_grid(&mut self, node_id: uuid::Uuid) -> Result<PointSceneSource, LibraryError> {
        let node = self
            .definition
            .nodes
            .get(&node_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Compiled Point Grid reaches missing Node {node_id}"
                ))
            })?;
        let values = self.node_values(&node)?;
        let parameters = PointGridParameters {
            counts: [
                required_u32(&values, "count_x", "Point Grid")?,
                required_u32(&values, "count_y", "Point Grid")?,
                required_u32(&values, "count_z", "Point Grid")?,
            ],
            spacing: required_vec3(&values, "spacing", "Point Grid")?,
            center: required_vec3(&values, "center", "Point Grid")?,
            size: finite_f32(
                required_number(&values, "size", "Point Grid")?,
                "Point Grid size",
            )?,
            seed: required_u32(&values, "seed", "Point Grid")?,
        };
        Ok(PointSceneSource::Grid(parameters))
    }

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
                        value_type,
                    } => {
                        let value = self.value_input(*node_id, port)?.ok_or_else(|| {
                            LibraryError::Validation(format!(
                                "Point uniform {node_id}:{port} produced no value"
                            ))
                        })?;
                        PointInstruction::Constant {
                            value: canonical_uniform(*value_type, value)?,
                        }
                    }
                    CompiledPointInstruction::Age => PointInstruction::Age,
                    CompiledPointInstruction::NormalizedAge => PointInstruction::NormalizedAge,
                    CompiledPointInstruction::Position => PointInstruction::Position,
                    CompiledPointInstruction::Random { channel } => {
                        PointInstruction::Random { channel: *channel }
                    }
                    CompiledPointInstruction::LoadAttribute { attribute } => {
                        PointInstruction::LoadAttribute {
                            attribute: *attribute,
                        }
                    }
                    CompiledPointInstruction::StoreAttribute { attribute, value } => {
                        PointInstruction::StoreAttribute {
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
                    CompiledPointInstruction::Length { value } => {
                        PointInstruction::Length { value: *value }
                    }
                    CompiledPointInstruction::Compare {
                        operation,
                        left,
                        right,
                    } => PointInstruction::Compare {
                        operation: *operation,
                        left: *left,
                        right: *right,
                    },
                    CompiledPointInstruction::Select {
                        element_type,
                        condition,
                        when_true,
                        when_false,
                    } => PointInstruction::Select {
                        element_type: *element_type,
                        condition: *condition,
                        when_true: *when_true,
                        when_false: *when_false,
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
            position_register: compiled.position_register,
        };
        program.validate().map_err(LibraryError::Validation)?;
        Ok(program)
    }
}

fn canonical_uniform(
    value_type: impl Into<CompiledPointValueType>,
    value: PropertyValue,
) -> Result<PropertyValue, LibraryError> {
    let kind = match value_type.into() {
        CompiledPointValueType::Exact(kind) => kind,
        CompiledPointValueType::Numeric => {
            let kind = PointAttributeElementType::from_property_value(&value)
                .map_err(LibraryError::Validation)?;
            if kind == PointAttributeElementType::Integer {
                // Uniform Numeric values use the existing numeric kernel's
                // Integer-to-Number semantics, unlike varying Integer fields.
                PointAttributeElementType::Number
            } else {
                kind.numeric_shape().map_err(LibraryError::Validation)?;
                kind
            }
        }
    };
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
