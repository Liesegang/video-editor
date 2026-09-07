//! Test-only inspection of the authoritative GPU Point buffers.

use glow::HasContext;

use super::*;
use crate::model::point::{PointAttributeElementType, PointAttributeGpuDefault};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PointFieldReadback {
    pub serial: u32,
    pub age: Option<f32>,
    pub lifetime: Option<f32>,
    /// Authoritative simulated position before render-stage fields. Grid has
    /// no mutable producer buffer, so its source position is `None` here.
    pub source_position: Option<[f32; 3]>,
    pub attributes: Vec<PointAttributeGpuDefault>,
    pub attribute_words: Vec<[u32; 4]>,
    pub color: [f32; 4],
    /// Derived producer-local position consumed by Sprite rendering. `None`
    /// proves the source geometry fast path remained active.
    pub position: Option<[f32; 3]>,
}

impl SceneRuntime {
    pub(crate) fn read_point_fields(
        &self,
        key: &SceneInvocationKey,
    ) -> Result<Vec<PointFieldReadback>, LibraryError> {
        let invocation = self.invocations.get(key).ok_or_else(|| {
            LibraryError::Render("Point readback has no matching invocation".to_string())
        })?;
        let fields = invocation.point_fields.as_ref().ok_or_else(|| {
            LibraryError::Render("Point readback has no render-stage program".to_string())
        })?;
        let mut particles = invocation
            .particle
            .as_ref()
            .map(|_| {
                usize::try_from(u64::from(invocation.capacity) * PARTICLE_STRIDE_BYTES)
                    .map(|bytes| vec![0_u8; bytes])
                    .map_err(|_| LibraryError::Render("Point readback size overflow".to_string()))
            })
            .transpose()?;
        let mut columns = vec![0_u8; fields.layout.byte_len as usize];
        let mut colors = vec![0_u8; invocation.capacity as usize * 16];
        let mut positions = fields
            .positions
            .map(|_| vec![0_u8; invocation.capacity as usize * 16]);
        let saved = SavedGlState::capture(&self.gl);
        // SAFETY: test-only inspection runs with SceneRuntime's context current;
        // every range exactly matches an owned buffer allocation.
        unsafe {
            if let (Some(particle), Some(bytes)) = (&invocation.particle, particles.as_mut()) {
                self.gl
                    .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(particle.buffer));
                self.gl
                    .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, bytes);
            }
            self.gl
                .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(fields.columns));
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut columns);
            self.gl
                .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(fields.colors));
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut colors);
            if let (Some(buffer), Some(bytes)) = (fields.positions, positions.as_mut()) {
                self.gl
                    .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
                self.gl
                    .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, bytes);
            }
        }
        saved.restore(&self.gl);
        gl_operation_result(&self.gl, "Point field test readback")?;
        let mut result = Vec::new();
        for slot in 0..invocation.capacity as usize {
            let (age, lifetime, source_position) = if let Some(particles) = &particles {
                let particle = slot * PARTICLE_STRIDE_BYTES as usize;
                let age = read_f32(particles, particle + 12);
                let lifetime = read_f32(particles, particle + 28);
                if age < 0.0 || age >= lifetime {
                    continue;
                }
                (
                    Some(age),
                    Some(lifetime),
                    Some(std::array::from_fn(|component| {
                        read_f32(particles, particle + component * 4)
                    })),
                )
            } else {
                (None, None, None)
            };
            let serial_offset = fields.layout.serial_offset_bytes as usize
                + slot * fields.layout.serial_stride_bytes as usize;
            let serial = read_u32(&columns, serial_offset);
            let attributes = fields
                .layout
                .attributes
                .iter()
                .map(|attribute| {
                    let offset =
                        attribute.offset_bytes as usize + slot * attribute.stride_bytes as usize;
                    read_attribute(&columns, offset, attribute.element_type)
                })
                .collect();
            let attribute_words = fields
                .layout
                .attributes
                .iter()
                .map(|attribute| {
                    let offset =
                        attribute.offset_bytes as usize + slot * attribute.stride_bytes as usize;
                    let mut words = [0; 4];
                    for (index, word) in words
                        .iter_mut()
                        .take(attribute.stride_bytes as usize / size_of::<u32>())
                        .enumerate()
                    {
                        *word = read_u32(&columns, offset + index * size_of::<u32>());
                    }
                    words
                })
                .collect();
            let color_offset = slot * 16;
            let color =
                std::array::from_fn(|component| read_f32(&colors, color_offset + component * 4));
            let position = positions.as_ref().map(|positions| {
                let offset = slot * 16;
                std::array::from_fn(|component| read_f32(positions, offset + component * 4))
            });
            result.push(PointFieldReadback {
                serial,
                age,
                lifetime,
                source_position,
                attributes,
                attribute_words,
                color,
                position,
            });
        }
        Ok(result)
    }

    pub(crate) fn point_invocation_has_particle_state(
        &self,
        key: &SceneInvocationKey,
    ) -> Option<bool> {
        self.invocations
            .get(key)
            .map(|invocation| invocation.particle.is_some())
    }

    pub(crate) fn compiled_point_pipeline_count(&self) -> usize {
        self.pipelines.len()
    }
}

fn read_attribute(
    bytes: &[u8],
    offset: usize,
    kind: PointAttributeElementType,
) -> PointAttributeGpuDefault {
    match kind {
        PointAttributeElementType::Number => {
            PointAttributeGpuDefault::Number(read_f32(bytes, offset))
        }
        PointAttributeElementType::Integer => {
            PointAttributeGpuDefault::Integer(read_u32(bytes, offset) as i32)
        }
        PointAttributeElementType::Boolean => {
            PointAttributeGpuDefault::Boolean(read_u32(bytes, offset) != 0)
        }
        PointAttributeElementType::Vec2 => {
            PointAttributeGpuDefault::Vec2([read_f32(bytes, offset), read_f32(bytes, offset + 4)])
        }
        PointAttributeElementType::Vec3 => PointAttributeGpuDefault::Vec3([
            read_f32(bytes, offset),
            read_f32(bytes, offset + 4),
            read_f32(bytes, offset + 8),
        ]),
        PointAttributeElementType::Vec4 => {
            PointAttributeGpuDefault::Vec4(std::array::from_fn(|component| {
                read_f32(bytes, offset + component * 4)
            }))
        }
        PointAttributeElementType::Color => {
            PointAttributeGpuDefault::Color(std::array::from_fn(|component| {
                read_f32(bytes, offset + component * 4)
            }))
        }
    }
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    f32::from_ne_bytes(value)
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_ne_bytes(value)
}
