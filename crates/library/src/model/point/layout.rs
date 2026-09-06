use crate::color_management::to_working_linear_srgb;
use crate::model::property::PropertyValue;

use super::schema::{default_type_mismatch, narrow_f32};
use super::{PointAttributeElementType, PointAttributeId, PointAttributeSchema};

pub const POINT_MAX_CAPACITY: u32 = 100_000;
pub const POINT_MAX_COLUMN_BYTES: u64 = 16 * 1024 * 1024;

const POINT_SERIAL_STRIDE_BYTES: u32 = size_of::<u32>() as u32;

/// Canonical GPU-side default after checked narrowing and color conversion.
/// This derived value is not serialized into the Project.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PointAttributeGpuDefault {
    Number(f32),
    Integer(i32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// Straight-alpha working-linear sRGB.
    Color([f32; 4]),
}

#[derive(Clone, PartialEq, Debug)]
pub struct PointAttributeColumnLayout {
    pub attribute_id: PointAttributeId,
    pub element_type: PointAttributeElementType,
    pub offset_bytes: u64,
    pub stride_bytes: u32,
    pub default_value: PointAttributeGpuDefault,
}

/// Deterministic packed column layout for one Point domain.
///
/// The exact `u32` serial column is distinct from custom attributes. Producer
/// identity remains a domain-level value and combines with this serial to form
/// a [`PointId`](super::PointId); neither slot index nor float data is an ID.
#[derive(Clone, PartialEq, Debug)]
pub struct PointColumnLayout {
    pub capacity: u32,
    pub serial_offset_bytes: u64,
    pub serial_stride_bytes: u32,
    pub attributes: Vec<PointAttributeColumnLayout>,
    pub byte_len: u64,
}

impl PointColumnLayout {
    pub fn derive(schema: &PointAttributeSchema, capacity: u32) -> Result<Self, String> {
        if capacity == 0 || capacity > POINT_MAX_CAPACITY {
            return Err(format!(
                "Point capacity must be between 1 and {POINT_MAX_CAPACITY}"
            ));
        }

        let serial_offset_bytes = 0;
        let mut cursor = checked_column_size(capacity, POINT_SERIAL_STRIDE_BYTES)?;
        let mut attributes = Vec::with_capacity(schema.attributes().len());
        for attribute in schema.attributes() {
            let (alignment, stride_bytes) = gpu_layout(attribute.element_type());
            cursor = align_up(cursor, alignment)?;
            let offset_bytes = cursor;
            cursor = cursor
                .checked_add(checked_column_size(capacity, stride_bytes)?)
                .ok_or_else(|| "Point column layout byte size overflowed".to_string())?;
            if cursor > POINT_MAX_COLUMN_BYTES {
                return Err(format!(
                    "Point column layout requires {cursor} bytes, exceeding the {POINT_MAX_COLUMN_BYTES}-byte limit"
                ));
            }
            attributes.push(PointAttributeColumnLayout {
                attribute_id: attribute.id(),
                element_type: attribute.element_type(),
                offset_bytes,
                stride_bytes,
                default_value: attribute
                    .element_type()
                    .pack_default(attribute.default_value())?,
            });
        }
        Ok(Self {
            capacity,
            serial_offset_bytes,
            serial_stride_bytes: POINT_SERIAL_STRIDE_BYTES,
            attributes,
            byte_len: cursor,
        })
    }
}

impl PointAttributeElementType {
    fn pack_default(self, value: &PropertyValue) -> Result<PointAttributeGpuDefault, String> {
        self.validate_authored_default(value)?;
        match (self, value) {
            (Self::Number, PropertyValue::Number(value)) => {
                Ok(PointAttributeGpuDefault::Number(value.into_inner() as f32))
            }
            (Self::Integer, PropertyValue::Integer(value)) => i32::try_from(*value)
                .map(PointAttributeGpuDefault::Integer)
                .map_err(|_| "Point Integer default must fit exactly in signed i32".to_string()),
            (Self::Vec2, PropertyValue::Vec2(value)) => Ok(PointAttributeGpuDefault::Vec2([
                value.x.into_inner() as f32,
                value.y.into_inner() as f32,
            ])),
            (Self::Vec3, PropertyValue::Vec3(value)) => Ok(PointAttributeGpuDefault::Vec3([
                value.x.into_inner() as f32,
                value.y.into_inner() as f32,
                value.z.into_inner() as f32,
            ])),
            (Self::Vec4, PropertyValue::Vec4(value)) => Ok(PointAttributeGpuDefault::Vec4([
                value.x.into_inner() as f32,
                value.y.into_inner() as f32,
                value.z.into_inner() as f32,
                value.w.into_inner() as f32,
            ])),
            (Self::Color, PropertyValue::ColorValue(value)) => {
                let working = to_working_linear_srgb(value)
                    .map_err(|error| format!("Point Color default cannot be converted: {error}"))?;
                let rgba = working.rgba();
                Ok(PointAttributeGpuDefault::Color([
                    narrow_component("r", rgba[0])?,
                    narrow_component("g", rgba[1])?,
                    narrow_component("b", rgba[2])?,
                    narrow_component("a", rgba[3])?,
                ]))
            }
            (expected, actual) => Err(default_type_mismatch(expected, actual)),
        }
    }
}

fn gpu_layout(element_type: PointAttributeElementType) -> (u64, u32) {
    match element_type {
        PointAttributeElementType::Number | PointAttributeElementType::Integer => (4, 4),
        PointAttributeElementType::Vec2 => (8, 8),
        PointAttributeElementType::Vec3
        | PointAttributeElementType::Vec4
        | PointAttributeElementType::Color => (16, 16),
    }
}

fn checked_column_size(capacity: u32, stride_bytes: u32) -> Result<u64, String> {
    u64::from(capacity)
        .checked_mul(u64::from(stride_bytes))
        .ok_or_else(|| "Point column byte size overflowed".to_string())
}

fn align_up(value: u64, alignment: u64) -> Result<u64, String> {
    let remainder = value % alignment;
    if remainder == 0 {
        return Ok(value);
    }
    value
        .checked_add(alignment - remainder)
        .ok_or_else(|| "Point column alignment overflowed".to_string())
}

fn narrow_component(component: &str, value: f64) -> Result<f32, String> {
    narrow_f32(value).map_err(|error| format!("component {component} {error}"))
}
