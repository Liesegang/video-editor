//! Bounded CPU packing for the render-stage Point program SSBO.

use crate::error::LibraryError;
use crate::model::point::{
    POINT_MAX_INSTRUCTIONS, POINT_MAX_RAMP_STOPS, POINT_MAX_RAMPS, PointAttributeElementType,
    PointAttributeGpuDefault, PointInstruction, PointRenderProgram,
};
use crate::model::property::{GradientSpread, PropertyValue};

pub(super) const PROGRAM_HEADER_VEC4S: usize = POINT_MAX_INSTRUCTIONS + POINT_MAX_RAMPS;
pub(super) const RAMP_STOP_VEC4S: usize = 2;
const PROGRAM_DATA_VEC4S: usize =
    PROGRAM_HEADER_VEC4S + POINT_MAX_RAMPS * POINT_MAX_RAMP_STOPS * RAMP_STOP_VEC4S;
pub(super) const PROGRAM_DATA_BYTES: usize = PROGRAM_DATA_VEC4S * 16;

pub(super) fn program_data(program: &PointRenderProgram) -> Result<Vec<u8>, LibraryError> {
    let mut values = vec![[0_u32; 4]; PROGRAM_DATA_VEC4S];
    for (index, instruction) in program.instructions.iter().enumerate() {
        if let PointInstruction::Constant { value } = instruction {
            values[index] = packed_value(value)?;
        }
    }
    for (ramp_index, ramp) in program.ramps.iter().enumerate() {
        values[POINT_MAX_INSTRUCTIONS + ramp_index] = [
            ramp.stops().len() as u32,
            match ramp.spread() {
                GradientSpread::Pad => 0,
                GradientSpread::Repeat => 1,
                GradientSpread::Reflect => 2,
            },
            0,
            0,
        ];
        let base = PROGRAM_HEADER_VEC4S + ramp_index * POINT_MAX_RAMP_STOPS * RAMP_STOP_VEC4S;
        for (stop_index, stop) in ramp.stops().iter().enumerate() {
            let color = packed_value(&PropertyValue::ColorValue(stop.color().clone()))?;
            values[base + stop_index * 2] = [
                (stop.offset() as f32).to_bits(),
                color[0],
                color[1],
                color[2],
            ];
            values[base + stop_index * 2 + 1] = [color[3], 0, 0, 0];
        }
    }
    let mut bytes = Vec::with_capacity(PROGRAM_DATA_BYTES);
    for value in values {
        for component in value {
            bytes.extend_from_slice(&component.to_ne_bytes());
        }
    }
    Ok(bytes)
}

fn packed_value(value: &PropertyValue) -> Result<[u32; 4], LibraryError> {
    let kind =
        PointAttributeElementType::from_property_value(value).map_err(LibraryError::Validation)?;
    match kind.pack_value(value).map_err(LibraryError::Validation)? {
        PointAttributeGpuDefault::Number(value) => Ok([value.to_bits(), 0, 0, 0]),
        PointAttributeGpuDefault::Integer(value) => Ok([value as u32, 0, 0, 0]),
        PointAttributeGpuDefault::Boolean(value) => Ok([u32::from(value), 0, 0, 0]),
        PointAttributeGpuDefault::Vec2(value) => Ok([value[0].to_bits(), value[1].to_bits(), 0, 0]),
        PointAttributeGpuDefault::Vec3(value) => Ok([
            value[0].to_bits(),
            value[1].to_bits(),
            value[2].to_bits(),
            0,
        ]),
        PointAttributeGpuDefault::Vec4(value) | PointAttributeGpuDefault::Color(value) => {
            Ok(value.map(f32::to_bits))
        }
    }
}
