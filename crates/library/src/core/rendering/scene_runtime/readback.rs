//! Test-only inspection of the authoritative GPU Point buffers.

use glow::HasContext;

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PointFieldReadback {
    pub serial: u32,
    pub age: f32,
    pub lifetime: f32,
    pub attributes: Vec<f32>,
    pub color: [f32; 4],
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
        let particle_bytes =
            usize::try_from(u64::from(invocation.capacity) * PARTICLE_STRIDE_BYTES)
                .map_err(|_| LibraryError::Render("Point readback size overflow".to_string()))?;
        let mut particles = vec![0_u8; particle_bytes];
        let mut columns = vec![0_u8; fields.layout.byte_len as usize];
        let mut colors = vec![0_u8; invocation.capacity as usize * 16];
        let saved = SavedGlState::capture(&self.gl);
        // SAFETY: test-only inspection runs with SceneRuntime's context current;
        // every range exactly matches an owned buffer allocation.
        unsafe {
            self.gl
                .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(invocation.buffer));
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut particles);
            self.gl
                .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(fields.columns));
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut columns);
            self.gl
                .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(fields.colors));
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut colors);
        }
        saved.restore(&self.gl);
        gl_operation_result(&self.gl, "Point field test readback")?;
        let mut result = Vec::new();
        for slot in 0..invocation.capacity as usize {
            let particle = slot * PARTICLE_STRIDE_BYTES as usize;
            let age = read_f32(&particles, particle + 12);
            let lifetime = read_f32(&particles, particle + 28);
            if age < 0.0 || age >= lifetime {
                continue;
            }
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
                    read_f32(&columns, offset)
                })
                .collect();
            let color_offset = slot * 16;
            let color =
                std::array::from_fn(|component| read_f32(&colors, color_offset + component * 4));
            result.push(PointFieldReadback {
                serial,
                age,
                lifetime,
                attributes,
                color,
            });
        }
        Ok(result)
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
