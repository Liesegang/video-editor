//! OpenGL resource ABI and state isolation for [`super::SceneRuntime`].

use crate::rendering::gl_resources::{GlTargetBindings, link_program};
use glow::HasContext;

use crate::error::LibraryError;

use super::shaders::{PARTICLE_COMPUTE, PARTICLE_FRAGMENT, PARTICLE_VERTEX};

pub(super) const PARTICLE_STRIDE_BYTES: u64 = 48;
pub(super) const PARTICLE_WORKGROUP_SIZE: u32 = 64;
pub(super) const PARTICLE_VERTICES_PER_SPRITE: u32 = 6;

#[derive(Clone, Debug)]
pub(super) struct CapabilityProfile {
    pub label: String,
    pub max_texture_size: u32,
}

pub(super) fn probe_capabilities(gl: &glow::Context) -> Result<CapabilityProfile, String> {
    let version = gl.version();
    // SAFETY: SceneRuntime constructs this backend only while its owning
    // glutin context is current; these calls only query that context.
    let (label, storage_bindings, workgroup_invocations, max_texture_size) = unsafe {
        (
            gl.get_parameter_string(glow::VERSION),
            gl.get_parameter_i32(glow::MAX_SHADER_STORAGE_BUFFER_BINDINGS),
            gl.get_parameter_i32(glow::MAX_COMPUTE_WORK_GROUP_INVOCATIONS),
            gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE),
        )
    };
    if version.is_embedded || (version.major, version.minor) < (4, 3) {
        return Err(format!(
            "GPU Particle requires desktop OpenGL 4.3 compute/SSBO support; active context is {label}"
        ));
    }
    if storage_bindings < 1 || workgroup_invocations < PARTICLE_WORKGROUP_SIZE as i32 {
        return Err(format!(
            "GPU Particle cannot run on {label}: available SSBO bindings={storage_bindings}, compute workgroup invocations={workgroup_invocations}"
        ));
    }
    let max_texture_size = u32::try_from(max_texture_size)
        .map_err(|_| format!("GPU Particle received invalid GL_MAX_TEXTURE_SIZE from {label}"))?;
    Ok(CapabilityProfile {
        label,
        max_texture_size,
    })
}

#[derive(Clone)]
pub(super) struct ComputeUniforms {
    pub capacity: glow::UniformLocation,
    pub reset: glow::UniformLocation,
    pub seed: glow::UniformLocation,
    pub start_step: glow::UniformLocation,
    pub step_count: glow::UniformLocation,
    pub rate: glow::UniformLocation,
    pub lifetime: glow::UniformLocation,
    pub velocity_min: glow::UniformLocation,
    pub velocity_max: glow::UniformLocation,
    pub gravity: glow::UniformLocation,
    pub drag: glow::UniformLocation,
    pub size_min: glow::UniformLocation,
    pub size_max: glow::UniformLocation,
}

#[derive(Clone)]
pub(super) struct RenderUniforms {
    pub logical_size: glow::UniformLocation,
    pub target_size: glow::UniformLocation,
    pub affine_x: glow::UniformLocation,
    pub affine_y: glow::UniformLocation,
    pub focal_length: glow::UniformLocation,
    pub premultiplied_color: glow::UniformLocation,
}

#[derive(Clone)]
pub(super) struct ParticlePipeline {
    pub compute_program: glow::Program,
    pub render_program: glow::Program,
    pub vertex_array: glow::VertexArray,
    pub compute: ComputeUniforms,
    pub render: RenderUniforms,
    pub last_used: u64,
}

impl ParticlePipeline {
    pub fn create(gl: &glow::Context, last_used: u64) -> Result<Self, LibraryError> {
        let compute_program =
            link_program(gl, &[(glow::COMPUTE_SHADER, PARTICLE_COMPUTE)], "compute")?;
        let render_program = match link_program(
            gl,
            &[
                (glow::VERTEX_SHADER, PARTICLE_VERTEX),
                (glow::FRAGMENT_SHADER, PARTICLE_FRAGMENT),
            ],
            "sprite",
        ) {
            Ok(program) => program,
            Err(error) => {
                // SAFETY: `compute_program` was created by this live context
                // above and has not been deleted or transferred.
                unsafe { gl.delete_program(compute_program) };
                return Err(error);
            }
        };
        // SAFETY: the owning glutin context is current for the complete
        // SceneRuntime call and no other thread accesses it concurrently.
        let vertex_array = match unsafe { gl.create_vertex_array() } {
            Ok(vertex_array) => vertex_array,
            Err(error) => {
                // SAFETY: both programs were created by this context above
                // and this error path is their sole owner.
                unsafe {
                    gl.delete_program(compute_program);
                    gl.delete_program(render_program);
                }
                return Err(LibraryError::Render(format!(
                    "Cannot create GPU Particle vertex array: {error}"
                )));
            }
        };
        let uniforms = (|| {
            Ok((
                ComputeUniforms {
                    capacity: required_uniform(gl, compute_program, "uCapacity")?,
                    reset: required_uniform(gl, compute_program, "uReset")?,
                    seed: required_uniform(gl, compute_program, "uSeed")?,
                    start_step: required_uniform(gl, compute_program, "uStartStep")?,
                    step_count: required_uniform(gl, compute_program, "uStepCount")?,
                    rate: required_uniform(gl, compute_program, "uRate")?,
                    lifetime: required_uniform(gl, compute_program, "uLifetime")?,
                    velocity_min: required_uniform(gl, compute_program, "uVelocityMin")?,
                    velocity_max: required_uniform(gl, compute_program, "uVelocityMax")?,
                    gravity: required_uniform(gl, compute_program, "uGravity")?,
                    drag: required_uniform(gl, compute_program, "uDrag")?,
                    size_min: required_uniform(gl, compute_program, "uSizeMin")?,
                    size_max: required_uniform(gl, compute_program, "uSizeMax")?,
                },
                RenderUniforms {
                    logical_size: required_uniform(gl, render_program, "uLogicalSize")?,
                    target_size: required_uniform(gl, render_program, "uTargetSize")?,
                    affine_x: required_uniform(gl, render_program, "uAffineX")?,
                    affine_y: required_uniform(gl, render_program, "uAffineY")?,
                    focal_length: required_uniform(gl, render_program, "uFocalLength")?,
                    premultiplied_color: required_uniform(
                        gl,
                        render_program,
                        "uPremultipliedColor",
                    )?,
                },
            ))
        })();
        let (compute, render) = match uniforms {
            Ok(uniforms) => uniforms,
            Err(error) => {
                // SAFETY: these three handles were created by this context and
                // have not escaped because pipeline construction failed.
                unsafe {
                    gl.delete_vertex_array(vertex_array);
                    gl.delete_program(compute_program);
                    gl.delete_program(render_program);
                }
                return Err(error);
            }
        };
        Ok(Self {
            compute_program,
            render_program,
            vertex_array,
            compute,
            render,
            last_used,
        })
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: ParticlePipeline uniquely owns handles created by `gl`; its
        // caller destroys it while that same context is current.
        unsafe {
            gl.delete_vertex_array(self.vertex_array);
            gl.delete_program(self.compute_program);
            gl.delete_program(self.render_program);
        }
    }
}

fn required_uniform(
    gl: &glow::Context,
    program: glow::Program,
    name: &str,
) -> Result<glow::UniformLocation, LibraryError> {
    // SAFETY: callers pass a successfully linked live program belonging to
    // the current context; this query does not mutate resource ownership.
    unsafe { gl.get_uniform_location(program, name) }.ok_or_else(|| {
        LibraryError::Render(format!(
            "GPU Particle built-in program omitted required uniform {name}"
        ))
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SceneTextureFormat {
    Srgba8,
    LinearRgbaF16,
    LinearRgbaF32,
}

impl SceneTextureFormat {
    pub fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Srgba8 => 4,
            Self::LinearRgbaF16 => 8,
            Self::LinearRgbaF32 => 16,
        }
    }

    fn gl_internal_format(self) -> u32 {
        match self {
            Self::Srgba8 => glow::RGBA8,
            Self::LinearRgbaF16 => glow::RGBA16F,
            Self::LinearRgbaF32 => glow::RGBA32F,
        }
    }
}

pub(super) struct SceneTarget {
    pub width: u32,
    pub height: u32,
    pub format: SceneTextureFormat,
    pub texture: glow::Texture,
    pub framebuffer: glow::Framebuffer,
}

impl SceneTarget {
    pub fn create(
        gl: &glow::Context,
        width: u32,
        height: u32,
        format: SceneTextureFormat,
    ) -> Result<Self, LibraryError> {
        // SAFETY: SceneRuntime owns an exclusively borrowed, current context
        // for target allocation.
        let texture = unsafe { gl.create_texture() }.map_err(|error| {
            LibraryError::Render(format!(
                "Cannot allocate GPU Particle target texture: {error}"
            ))
        })?;
        // SAFETY: the same context remains current and the texture allocation
        // above does not change context ownership.
        let framebuffer = match unsafe { gl.create_framebuffer() } {
            Ok(framebuffer) => framebuffer,
            Err(error) => {
                // SAFETY: this error path exclusively owns the live texture.
                unsafe { gl.delete_texture(texture) };
                return Err(LibraryError::Render(format!(
                    "Cannot allocate GPU Particle framebuffer: {error}"
                )));
            }
        };
        let result = (|| {
            // SAFETY: both bound names were created by this current context;
            // dimensions and formats were validated by SceneRuntime before
            // construction, and storage is initialized before attachment.
            unsafe {
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_storage_2d(
                    glow::TEXTURE_2D,
                    1,
                    format.gl_internal_format(),
                    width as i32,
                    height as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
                if status != glow::FRAMEBUFFER_COMPLETE {
                    return Err(LibraryError::Render(format!(
                        "GPU Particle framebuffer is incomplete (OpenGL status 0x{status:04x})"
                    )));
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            // SAFETY: failed construction still exclusively owns both
            // handles and deletes each exactly once.
            unsafe {
                gl.delete_framebuffer(framebuffer);
                gl.delete_texture(texture);
            }
            return Err(error);
        }
        Ok(Self {
            width,
            height,
            format,
            texture,
            framebuffer,
        })
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: SceneTarget uniquely owns resources created by this context
        // and destruction runs while that same context is current.
        unsafe {
            gl.delete_framebuffer(self.framebuffer);
            gl.delete_texture(self.texture);
        }
    }

    pub fn texture_id(&self) -> u32 {
        self.texture.0.get()
    }

    pub fn bindings(&self) -> GlTargetBindings {
        GlTargetBindings {
            texture_id: self.texture.0.get(),
            framebuffer_id: self.framebuffer.0.get(),
        }
    }
}
