//! Final Project color transform on the renderer's existing OpenGL owner.
//!
//! Input stays premultiplied floating point on Ganesh. Only the final straight
//! RGBA8 bytes and an invalid-pixel index cross back to the CPU. No texture or
//! context escapes this owner; unsupported hardware is decided before drawing.

use glow::HasContext;
use ruvie_color_management::{
    CompiledTransformIdentity, GpuShaderLanguage, GpuTerminalChain, WorkingColorIdentity,
};

use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::rendering::gl_resources::{SavedGlState, link_program};

const WORKGROUP: u32 = 8;
const VALID: u32 = u32::MAX;

struct Program {
    identity: (WorkingColorIdentity, Vec<CompiledTransformIdentity>),
    handle: glow::Program,
}

pub(super) struct TerminalCompute {
    gl: glow::Context,
    max_buffer_bytes: i64,
    max_workgroups: [u32; 2],
    program: Option<Program>,
    buffer: Option<glow::Buffer>,
    buffer_bytes: i32,
    #[cfg(test)]
    pub(super) compilations: usize,
}

impl TerminalCompute {
    /// The function table refers to the same current context as Ganesh. It
    /// neither allocates a second device nor claims unsupported capabilities.
    pub(super) fn new(gl: glow::Context) -> Option<Self> {
        let version = gl.version();
        if version.is_embedded || (version.major, version.minor) < (4, 3) {
            return None;
        }
        // SAFETY: the renderer activates its owner before constructing us;
        // all queried compute/SSBO capabilities belong to desktop GL 4.3.
        let (max_buffer_bytes, max_workgroups) = unsafe {
            if gl.get_parameter_i32(glow::MAX_COMPUTE_WORK_GROUP_INVOCATIONS) < 64
                || gl.get_parameter_i32(glow::MAX_COMPUTE_SHADER_STORAGE_BLOCKS) < 1
                || gl.get_parameter_i32(glow::MAX_COMPUTE_TEXTURE_IMAGE_UNITS) < 1
            {
                return None;
            }
            (
                gl.get_parameter_i64(glow::MAX_SHADER_STORAGE_BLOCK_SIZE),
                [0, 1].map(|axis| {
                    gl.get_parameter_indexed_i32(glow::MAX_COMPUTE_WORK_GROUP_COUNT, axis) as u32
                }),
            )
        };
        Some(Self {
            gl,
            max_buffer_bytes,
            max_workgroups,
            program: None,
            buffer: None,
            buffer_bytes: 0,
            #[cfg(test)]
            compilations: 0,
        })
    }

    pub(super) fn supports(&self, chain: &GpuTerminalChain, width: u32, height: u32) -> bool {
        chain.language() == GpuShaderLanguage::Glsl
            && chain.stages().iter().all(|stage| stage.luts().is_empty())
            && buffer_size(width, height)
                .is_some_and(|bytes| i64::from(bytes) <= self.max_buffer_bytes)
            && width.div_ceil(WORKGROUP) <= self.max_workgroups[0]
            && height.div_ceil(WORKGROUP) <= self.max_workgroups[1]
    }

    pub(super) fn render(
        &mut self,
        chain: &GpuTerminalChain,
        texture: u32,
        width: u32,
        height: u32,
    ) -> Result<Image, LibraryError> {
        if !self.supports(chain, width, height) {
            return Err(LibraryError::Render(
                "GPU terminal transform exceeded its validated capability".to_string(),
            ));
        }
        let saved = SavedGlState::capture(&self.gl);
        let result = self.render_isolated(chain, texture, width, height);
        saved.restore(&self.gl);
        result
    }

    fn render_isolated(
        &mut self,
        chain: &GpuTerminalChain,
        texture: u32,
        width: u32,
        height: u32,
    ) -> Result<Image, LibraryError> {
        let identity = (
            chain.working_identity().clone(),
            chain
                .stages()
                .iter()
                .map(|stage| stage.compiled_transform_identity().clone())
                .collect(),
        );
        if self
            .program
            .as_ref()
            .is_none_or(|program| program.identity != identity)
        {
            let shader = shader_source(chain);
            let handle = link_program(
                &self.gl,
                &[(glow::COMPUTE_SHADER, &shader)],
                "terminal color",
            )?;
            if let Some(previous) = self.program.replace(Program { identity, handle }) {
                // SAFETY: this stage uniquely owns the old linked program and
                // the caller keeps its context current through replacement.
                unsafe { self.gl.delete_program(previous.handle) };
            }
            #[cfg(test)]
            {
                self.compilations += 1;
            }
        }
        let bytes = buffer_size(width, height)
            .ok_or_else(|| LibraryError::Render("GPU terminal dimensions overflow".to_string()))?;
        if self.buffer.is_none() {
            // SAFETY: allocation occurs inside the same isolated current GL
            // context. The handle is retained and deleted by this owner.
            self.buffer = Some(unsafe { self.gl.create_buffer() }.map_err(LibraryError::Render)?);
        }
        let program = self
            .program
            .as_ref()
            .ok_or_else(|| LibraryError::Render("GPU terminal program missing".to_string()))?
            .handle;
        let texture = std::num::NonZeroU32::new(texture)
            .map(glow::NativeTexture)
            .ok_or_else(|| LibraryError::Render("GPU working texture is zero".to_string()))?;
        // SAFETY: all handles are live under the renderer's current context.
        // The allocation bounds were checked against GL and host limits; the
        // shader writes one uint per valid pixel plus the first-invalid index.
        unsafe {
            self.gl
                .bind_buffer(glow::SHADER_STORAGE_BUFFER, self.buffer);
            if self.buffer_bytes != bytes {
                self.gl
                    .buffer_data_size(glow::SHADER_STORAGE_BUFFER, bytes, glow::STREAM_READ);
                if self.gl.get_error() != glow::NO_ERROR
                    || self
                        .gl
                        .get_buffer_parameter_i32(glow::SHADER_STORAGE_BUFFER, glow::BUFFER_SIZE)
                        != bytes
                {
                    return Err(LibraryError::Render(format!(
                        "Cannot allocate GPU terminal output buffer ({bytes} bytes)"
                    )));
                }
                self.buffer_bytes = bytes;
            }
            self.gl
                .buffer_sub_data_u8_slice(glow::SHADER_STORAGE_BUFFER, 0, &VALID.to_ne_bytes());
            self.gl
                .bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, self.buffer);
            let unit = self.gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32 - glow::TEXTURE0;
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.bind_sampler(unit, None);
            self.gl.use_program(Some(program));
            self.gl.uniform_1_i32(
                self.gl
                    .get_uniform_location(program, "working_texture")
                    .as_ref(),
                unit as i32,
            );
            self.gl.uniform_2_u32(
                self.gl.get_uniform_location(program, "dimensions").as_ref(),
                width,
                height,
            );
            self.gl
                .dispatch_compute(width.div_ceil(WORKGROUP), height.div_ceil(WORKGROUP), 1);
            // GetBufferSubData is a buffer-update consumer of shader writes.
            // https://registry.khronos.org/OpenGL-Refpages/gl4/html/glMemoryBarrier.xhtml
            self.gl.memory_barrier(glow::BUFFER_UPDATE_BARRIER_BIT);
            let mut invalid = [0; 4];
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut invalid);
            let invalid = u32::from_ne_bytes(invalid);
            if invalid != VALID {
                return Err(LibraryError::Render(format!(
                    "GPU terminal color rejected invalid working/transform value at pixel {invalid}"
                )));
            }
            let mut rgba = Vec::new();
            rgba.try_reserve_exact((bytes - 4) as usize).map_err(|_| {
                LibraryError::Render("Cannot allocate terminal RGBA8 output".to_string())
            })?;
            rgba.resize((bytes - 4) as usize, 0);
            self.gl
                .get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 4, &mut rgba);
            #[cfg(target_endian = "big")]
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.reverse();
            }
            let error = self.gl.get_error();
            if error != glow::NO_ERROR {
                return Err(LibraryError::Render(format!(
                    "GPU terminal color failed with OpenGL error {error:#x}"
                )));
            }
            Ok(Image::new(width, height, rgba))
        }
    }
}

impl Drop for TerminalCompute {
    fn drop(&mut self) {
        // SAFETY: SkiaRenderer destroys native stages before replacing or
        // dropping their owning context, with that owner current.
        unsafe {
            if let Some(program) = self.program.take() {
                self.gl.delete_program(program.handle);
            }
            if let Some(buffer) = self.buffer.take() {
                self.gl.delete_buffer(buffer);
            }
        }
    }
}

fn buffer_size(width: u32, height: u32) -> Option<i32> {
    if width == 0 || height == 0 {
        return None;
    }
    u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_add(1)?
        .checked_mul(4)?
        .try_into()
        .ok()
}

fn shader_source(chain: &GpuTerminalChain) -> String {
    let mut source = String::from("#version 430 core\n");
    let mut seen = std::collections::HashSet::new();
    for stage in chain.stages() {
        if seen.insert(stage.compiled_transform_identity()) {
            source.push_str(stage.source());
            source.push('\n');
        }
    }
    source.push_str(
        r#"
layout(local_size_x=8, local_size_y=8) in;
uniform sampler2D working_texture;
uniform uvec2 dimensions;
layout(std430, binding=0) buffer TerminalOutput { uint invalid_pixel; uint pixels[]; };
bool finite3(vec3 rgb) { return !any(isnan(rgb)) && !any(isinf(rgb)); }
void main() {
    uvec2 p = gl_GlobalInvocationID.xy;
    if (any(greaterThanEqual(p, dimensions))) return;
    uint i = p.y * dimensions.x + p.x;
    vec4 working = texelFetch(working_texture, ivec2(p), 0);
    if (any(isnan(working)) || any(isinf(working)) || working.a < 0.0 || working.a > 1.0) {
        atomicMin(invalid_pixel, i); return;
    }
    vec3 rgb = working.a == 0.0 ? vec3(0.0) : working.rgb / working.a;
"#,
    );
    for stage in chain.stages() {
        source.push_str(&format!("    if (!finite3(rgb) || !{}(rgb)) {{ atomicMin(invalid_pixel, i); return; }}\n    rgb = {}(rgb);\n", stage.domain_entry_point(), stage.entry_point()));
    }
    source.push_str(
        r#"
    if (!finite3(rgb)) { atomicMin(invalid_pixel, i); return; }
    if (working.a == 0.0) rgb = vec3(0.0);
    uvec4 encoded = uvec4(floor(clamp(vec4(rgb, working.a), 0.0, 1.0) * 255.0 + 0.5));
    pixels[i] = encoded.r | (encoded.g << 8) | (encoded.b << 16) | (encoded.a << 24);
}
"#,
    );
    source
}
