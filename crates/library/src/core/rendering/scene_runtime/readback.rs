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
    pub source_velocity: Option<[f32; 3]>,
    /// Simulated birth size, before render-stage size fields. Grid has no
    /// mutable source buffer to read back.
    pub source_size: Option<f32>,
    pub attributes: Vec<PointAttributeGpuDefault>,
    pub attribute_words: Vec<[u32; 4]>,
    pub color: [f32; 4],
    /// Derived producer-local geometry consumed by Sprite rendering. Both
    /// position and size are present whenever the geometry output is active.
    pub position: Option<[f32; 3]>,
    pub size: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointConnectionReadback {
    pub source_serial: u32,
    pub target_serial: u32,
    pub distance: f32,
}

impl SceneRuntime {
    /// Read the compact topology already produced by the GPU. Positions and
    /// identities are resolved from the same current source/derived buffers;
    /// this never repeats proximity search on the CPU.
    pub(crate) fn read_point_connections(
        &self,
        scene: &PointSceneFrame,
    ) -> Result<Vec<PointConnectionReadback>, LibraryError> {
        let invocation = self.invocations.get(&scene.invocation).ok_or_else(|| {
            LibraryError::Render("Point connection readback has no matching invocation".into())
        })?;
        if !invocation.source_matches(scene) {
            return Err(LibraryError::Validation(
                "Point connection readback source does not match the invocation".into(),
            ));
        }
        if scene.source.capacity() != invocation.capacity {
            return Err(LibraryError::Validation(
                "Point connection readback capacity does not match the invocation".into(),
            ));
        }
        if invocation.connection_source.as_ref() != Some(&scene.source) {
            return Err(LibraryError::Validation(
                "Point connection readback frame is not the last evaluated source".into(),
            ));
        }
        let connections = invocation.connections.as_ref().ok_or_else(|| {
            LibraryError::Render("Point readback has no connection topology".into())
        })?;
        let saved = SavedGlState::capture(&self.gl);
        let edge_count = proximity::read_indirect_count(&self.gl, connections.scan.indirect);
        saved.restore(&self.gl);
        let edge_count = edge_count?;
        let maximum_edges = invocation
            .capacity
            .checked_mul(connections.max_neighbors)
            .map(|slots| slots.div_ceil(2))
            .ok_or_else(|| LibraryError::Render("Point edge readback limit overflow".into()))?;
        if edge_count > maximum_edges {
            return Err(LibraryError::Render(format!(
                "Point compact edge count {edge_count} exceeds its {maximum_edges}-edge allocation"
            )));
        }
        let edge_bytes = usize::try_from(u64::from(edge_count) * 8)
            .map_err(|_| LibraryError::Render("Point edge readback size overflow".into()))?;
        let mut edges = vec![0_u8; edge_bytes];
        let mut particles = invocation
            .particle
            .as_ref()
            .map(|_| {
                usize::try_from(u64::from(invocation.capacity) * PARTICLE_STRIDE_BYTES)
                    .map(|bytes| vec![0_u8; bytes])
                    .map_err(|_| LibraryError::Render("Point source readback size overflow".into()))
            })
            .transpose()?;
        let mut geometry = invocation
            .point_fields
            .as_ref()
            .and_then(|fields| fields.geometry)
            .map(|_| vec![0_u8; invocation.capacity as usize * 16]);
        let saved = SavedGlState::capture(&self.gl);
        // SAFETY: all ranges exactly match live buffers owned by this
        // invocation, and the caller has made SceneRuntime's context current.
        unsafe {
            self.gl.bind_buffer(
                glow::SHADER_STORAGE_BUFFER,
                Some(connections.scan.compact_edges),
            );
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut edges);
            if let (Some(particle), Some(bytes)) = (&invocation.particle, particles.as_mut()) {
                self.gl
                    .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(particle.buffer));
                self.gl
                    .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, bytes);
            }
            if let (Some(buffer), Some(bytes)) = (
                invocation
                    .point_fields
                    .as_ref()
                    .and_then(|fields| fields.geometry),
                geometry.as_mut(),
            ) {
                self.gl
                    .bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
                self.gl
                    .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, bytes);
            }
        }
        saved.restore(&self.gl);
        gl_operation_result(&self.gl, "Point connection test readback")?;

        (0..edge_count as usize)
            .map(|index| {
                let source_slot = read_u32(&edges, index * 8);
                let target_slot = read_u32(&edges, index * 8 + 4);
                if source_slot >= invocation.capacity || target_slot >= invocation.capacity {
                    return Err(LibraryError::Render(
                        "Point compact edge references an invalid source slot".into(),
                    ));
                }
                let (source_serial, source_position) = point_identity_position(
                    scene,
                    source_slot,
                    particles.as_deref(),
                    geometry.as_deref(),
                )?;
                let (target_serial, target_position) = point_identity_position(
                    scene,
                    target_slot,
                    particles.as_deref(),
                    geometry.as_deref(),
                )?;
                let delta = std::array::from_fn::<_, 3, _>(|axis| {
                    target_position[axis] - source_position[axis]
                });
                Ok(PointConnectionReadback {
                    source_serial,
                    target_serial,
                    distance: (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2])
                        .sqrt(),
                })
            })
            .collect()
    }

    /// Inspect one owned render-target pixel before any Ganesh format conversion.
    pub(crate) fn read_target_pixel_f32(&self, x: u32, y: u32) -> Result<[f32; 4], LibraryError> {
        let target = self.target.as_ref().ok_or_else(|| {
            LibraryError::Render("Point pixel readback has no render target".into())
        })?;
        if x >= target.width || y >= target.height {
            return Err(LibraryError::Render(
                "Point pixel readback is outside the target".into(),
            ));
        }
        let saved = SavedGlState::capture(&self.gl);
        let mut pixel = [0.0_f32; 4];
        // SAFETY: this test-only inspection owns the current context and reads
        // exactly one pixel from its live framebuffer into four f32 components.
        // Pack state and external framebuffer bindings are restored afterwards.
        unsafe {
            let pack_buffer = self
                .gl
                .get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING);
            let pack_parameters = [
                glow::PACK_ALIGNMENT,
                glow::PACK_ROW_LENGTH,
                glow::PACK_IMAGE_HEIGHT,
                glow::PACK_SKIP_ROWS,
                glow::PACK_SKIP_PIXELS,
                glow::PACK_SKIP_IMAGES,
                glow::PACK_SWAP_BYTES,
                glow::PACK_LSB_FIRST,
            ];
            let pack_values = pack_parameters.map(|parameter| self.gl.get_parameter_i32(parameter));
            self.gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            for parameter in pack_parameters {
                self.gl.pixel_store_i32(
                    parameter,
                    if parameter == glow::PACK_ALIGNMENT {
                        4
                    } else {
                        0
                    },
                );
            }
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
            self.gl.read_pixels(
                x as i32,
                y as i32,
                1,
                1,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelPackData::Slice(Some(bytemuck::cast_slice_mut(&mut pixel))),
            );
            self.gl.bind_buffer(glow::PIXEL_PACK_BUFFER, pack_buffer);
            for (parameter, value) in pack_parameters.into_iter().zip(pack_values) {
                self.gl.pixel_store_i32(parameter, value);
            }
        }
        saved.restore(&self.gl);
        gl_operation_result(&self.gl, "Point pixel test readback")?;
        Ok(pixel)
    }

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
        let mut geometry = fields
            .geometry
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
            if let (Some(buffer), Some(bytes)) = (fields.geometry, geometry.as_mut()) {
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
            let (age, lifetime, source_position, source_velocity, source_size) =
                if let Some(particles) = &particles {
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
                        Some(std::array::from_fn(|component| {
                            read_f32(particles, particle + 16 + component * 4)
                        })),
                        Some(read_f32(particles, particle + 44)),
                    )
                } else {
                    (None, None, None, None, None)
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
            let position = geometry.as_ref().map(|geometry| {
                let offset = slot * 16;
                std::array::from_fn(|component| read_f32(geometry, offset + component * 4))
            });
            let size = geometry
                .as_ref()
                .map(|geometry| read_f32(geometry, slot * 16 + 12));
            result.push(PointFieldReadback {
                serial,
                age,
                lifetime,
                source_position,
                source_velocity,
                source_size,
                attributes,
                attribute_words,
                color,
                position,
                size,
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

fn point_identity_position(
    scene: &PointSceneFrame,
    slot: u32,
    particles: Option<&[u8]>,
    geometry: Option<&[u8]>,
) -> Result<(u32, [f32; 3]), LibraryError> {
    let position = geometry.map(|bytes| {
        let offset = slot as usize * 16;
        std::array::from_fn(|axis| read_f32(bytes, offset + axis * 4))
    });
    match (&scene.source, particles) {
        (PointSceneSource::Particle { .. }, Some(bytes)) => {
            let offset = slot as usize * PARTICLE_STRIDE_BYTES as usize;
            let serial = read_u32(bytes, offset + 48);
            let source_position = std::array::from_fn(|axis| read_f32(bytes, offset + axis * 4));
            Ok((serial, position.unwrap_or(source_position)))
        }
        (PointSceneSource::Grid(grid), None) => {
            let xy = grid.counts[0] * grid.counts[1];
            let z = slot / xy;
            let remainder = slot - z * xy;
            let y = remainder / grid.counts[0];
            let x = remainder - y * grid.counts[0];
            let coordinate = [x, y, z];
            let serial = grid.point_serial(coordinate).ok_or_else(|| {
                LibraryError::Render("Point Grid edge has an invalid coordinate".into())
            })?;
            let center = [
                grid.center.x.0 as f32,
                grid.center.y.0 as f32,
                grid.center.z.0 as f32,
            ];
            let spacing = [
                grid.spacing.x.0 as f32,
                grid.spacing.y.0 as f32,
                grid.spacing.z.0 as f32,
            ];
            let source_position = std::array::from_fn(|axis| {
                let centered = coordinate[axis] as f32 - (grid.counts[axis] as f32 - 1.0) * 0.5;
                center[axis] + centered * spacing[axis]
            });
            Ok((serial, position.unwrap_or(source_position)))
        }
        _ => Err(LibraryError::Validation(
            "Point connection readback source storage is inconsistent".into(),
        )),
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
