//! Checked allocation and byte accounting shared by Point-derived buffers.

use glow::HasContext;

use super::drain_gl_errors;
use super::point_fields::{
    COLOR_STRIDE_BYTES, GEOMETRY_STRIDE_BYTES, PointFieldRequirements,
    SPRITE_SELECTION_STRIDE_BYTES, VALIDITY_STRIDE_BYTES,
};
use crate::error::LibraryError;
use crate::model::point::{PointColumnLayout, PointRenderProgram};

pub(super) fn required_invocation_bytes(
    has_particle_state: bool,
    program: Option<&PointRenderProgram>,
    capacity: u32,
    requirements: PointFieldRequirements,
) -> Result<u64, LibraryError> {
    let particle_bytes = if has_particle_state {
        u64::from(capacity)
            .checked_mul(super::gl_backend::PARTICLE_STRIDE_BYTES)
            .ok_or_else(|| LibraryError::Render("GPU Particle state size overflow".into()))?
    } else {
        0
    };
    let Some(program) = program else {
        return Ok(particle_bytes);
    };
    let layout =
        PointColumnLayout::derive(&program.schema, capacity).map_err(LibraryError::Validation)?;
    let colors = color_bytes(capacity)?;
    let geometry = geometry_bytes(program, capacity)?.unwrap_or(0);
    let selection = sprite_selection_bytes(program, capacity)?.unwrap_or(0);
    let validity = requirements
        .validity
        .then(|| validity_bytes(capacity))
        .transpose()?
        .unwrap_or(0);
    particle_bytes
        .checked_add(layout.byte_len)
        .and_then(|bytes| bytes.checked_add(colors))
        .and_then(|bytes| bytes.checked_add(geometry))
        .and_then(|bytes| bytes.checked_add(selection))
        .and_then(|bytes| bytes.checked_add(validity))
        .ok_or_else(|| LibraryError::Render("GPU Point field state size overflow".into()))
}

pub(super) fn color_bytes(capacity: u32) -> Result<u64, LibraryError> {
    u64::from(capacity)
        .checked_mul(COLOR_STRIDE_BYTES)
        .ok_or_else(|| LibraryError::Render("GPU Point color buffer size overflow".into()))
}

pub(super) fn geometry_bytes(
    program: &PointRenderProgram,
    capacity: u32,
) -> Result<Option<u64>, LibraryError> {
    program
        .has_geometry_output()
        .then(|| {
            u64::from(capacity)
                .checked_mul(GEOMETRY_STRIDE_BYTES)
                .ok_or_else(|| {
                    LibraryError::Render("GPU Point geometry buffer size overflow".into())
                })
        })
        .transpose()
}

pub(super) fn sprite_selection_bytes(
    program: &PointRenderProgram,
    capacity: u32,
) -> Result<Option<u64>, LibraryError> {
    program
        .sprite_selection_register
        .is_some()
        .then(|| {
            u64::from(capacity)
                .checked_mul(SPRITE_SELECTION_STRIDE_BYTES)
                .ok_or_else(|| {
                    LibraryError::Render("GPU Point Sprite selection buffer size overflow".into())
                })
        })
        .transpose()
}

pub(super) fn validity_bytes(capacity: u32) -> Result<u64, LibraryError> {
    u64::from(capacity)
        .checked_mul(VALIDITY_STRIDE_BYTES)
        .ok_or_else(|| LibraryError::Render("GPU Point validity size overflow".into()))
}

pub(super) fn allocate_buffer(
    gl: &glow::Context,
    bytes: u64,
    usage: u32,
    label: &str,
) -> Result<glow::Buffer, LibraryError> {
    let bytes = i32::try_from(bytes)
        .map_err(|_| LibraryError::Render(format!("{label} size exceeds the GPU range")))?;
    // SAFETY: SceneRuntime owns the current context and checked allocation.
    let buffer = unsafe { gl.create_buffer() }
        .map_err(|error| LibraryError::Render(format!("Cannot create {label}: {error}")))?;
    // SAFETY: the new buffer remains exclusively owned during allocation.
    unsafe {
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
        gl.buffer_data_size(glow::SHADER_STORAGE_BUFFER, bytes, usage);
    }
    let errors = drain_gl_errors(gl);
    if errors.is_empty() {
        return Ok(buffer);
    }
    // SAFETY: failed allocation retains sole ownership of this name.
    unsafe { gl.delete_buffer(buffer) };
    Err(LibraryError::Render(format!(
        "{label} allocation failed (OpenGL errors {})",
        errors
            .iter()
            .map(|error| format!("0x{error:04x}"))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}
