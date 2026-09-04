//! OpenGL resource ABI and state isolation for [`super::SceneRuntime`].

use std::num::NonZeroU32;

use glow::HasContext;

use crate::error::LibraryError;

use super::shaders::{PARTICLE_COMPUTE, PARTICLE_FRAGMENT, PARTICLE_VERTEX};

pub(super) const PARTICLE_STRIDE_BYTES: u64 = 48;
pub(super) const PARTICLE_WORKGROUP_SIZE: u32 = 64;

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
    pub point_scale: glow::UniformLocation,
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
                    point_scale: required_uniform(gl, render_program, "uPointScale")?,
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

fn link_program(
    gl: &glow::Context,
    stages: &[(u32, &str)],
    label: &str,
) -> Result<glow::Program, LibraryError> {
    // SAFETY: SceneRuntime calls pipeline construction with its current and
    // exclusively borrowed OpenGL context.
    let program = unsafe { gl.create_program() }.map_err(|error| {
        LibraryError::Render(format!(
            "Cannot create GPU Particle {label} program: {error}"
        ))
    })?;
    let mut shaders = Vec::with_capacity(stages.len());
    for (stage, source) in stages {
        // SAFETY: `stage` is one of the fixed shader stage constants supplied
        // by this module and the context remains current.
        let shader = match unsafe { gl.create_shader(*stage) } {
            Ok(shader) => shader,
            Err(error) => {
                destroy_program_build(gl, program, shaders);
                return Err(LibraryError::Render(format!(
                    "Cannot create GPU Particle {label} shader: {error}"
                )));
            }
        };
        // SAFETY: `shader` is a live handle created immediately above; source
        // is retained by glow for the duration of this call.
        unsafe {
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
        }
        // SAFETY: querying the compile result and log is valid for the live
        // shader handle until it is deleted below.
        if !unsafe { gl.get_shader_compile_status(shader) } {
            // SAFETY: same live shader and current context as the status query.
            let log = unsafe { gl.get_shader_info_log(shader) };
            shaders.push(shader);
            destroy_program_build(gl, program, shaders);
            return Err(LibraryError::Render(format!(
                "GPU Particle {label} shader compilation failed: {log}"
            )));
        }
        // SAFETY: both handles are live and owned by this in-progress build.
        unsafe { gl.attach_shader(program, shader) };
        shaders.push(shader);
    }
    // SAFETY: every attached shader above compiled successfully and remains
    // alive in `shaders` during this link operation.
    unsafe { gl.link_program(program) };
    // SAFETY: `program` remains live until the cleanup paths below.
    if !unsafe { gl.get_program_link_status(program) } {
        // SAFETY: reading the log is valid for this live program.
        let log = unsafe { gl.get_program_info_log(program) };
        destroy_program_build(gl, program, shaders);
        return Err(LibraryError::Render(format!(
            "GPU Particle {label} program link failed: {log}"
        )));
    }
    for shader in shaders {
        // SAFETY: each shader is still attached to `program`, both handles
        // belong to this context, and each is deleted exactly once here.
        unsafe {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
    }
    Ok(program)
}

fn destroy_program_build(gl: &glow::Context, program: glow::Program, shaders: Vec<glow::Shader>) {
    for shader in shaders {
        // SAFETY: this cleanup owns every supplied shader and the program;
        // handles have not escaped the failed build.
        unsafe {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
    }
    // SAFETY: this cleanup exclusively owns the failed program handle.
    unsafe { gl.delete_program(program) };
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
    LinearRgbaF32,
}

impl SceneTextureFormat {
    pub fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Srgba8 => 4,
            Self::LinearRgbaF32 => 16,
        }
    }

    fn gl_internal_format(self) -> u32 {
        match self {
            Self::Srgba8 => glow::RGBA8,
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
    pub depth: glow::Renderbuffer,
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
        // SAFETY: resource creation is issued to the same current context.
        let depth = match unsafe { gl.create_renderbuffer() } {
            Ok(depth) => depth,
            Err(error) => {
                // SAFETY: neither handle escaped this failed construction and
                // both belong to the current context.
                unsafe {
                    gl.delete_framebuffer(framebuffer);
                    gl.delete_texture(texture);
                }
                return Err(LibraryError::Render(format!(
                    "Cannot allocate GPU Particle depth buffer: {error}"
                )));
            }
        };
        let result = (|| {
            // SAFETY: all bound names were created by this current context;
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
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
                gl.renderbuffer_storage(
                    glow::RENDERBUFFER,
                    glow::DEPTH_COMPONENT24,
                    width as i32,
                    height as i32,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::DEPTH_ATTACHMENT,
                    glow::RENDERBUFFER,
                    Some(depth),
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
            // SAFETY: failed construction still exclusively owns all three
            // handles and deletes each exactly once.
            unsafe {
                gl.delete_renderbuffer(depth);
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
            depth,
        })
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: SceneTarget uniquely owns resources created by this context
        // and destruction runs while that same context is current.
        unsafe {
            gl.delete_renderbuffer(self.depth);
            gl.delete_framebuffer(self.framebuffer);
            gl.delete_texture(self.texture);
        }
    }

    pub fn texture_id(&self) -> u32 {
        self.texture.0.get()
    }
}

/// Exact subset of OpenGL state touched by SceneRuntime. The caller flushes
/// Ganesh first and resets its state cache after this snapshot is restored.
pub(super) struct SavedGlState {
    program: Option<glow::Program>,
    vertex_array: Option<glow::VertexArray>,
    draw_framebuffer: Option<glow::Framebuffer>,
    read_framebuffer: Option<glow::Framebuffer>,
    renderbuffer: Option<glow::Renderbuffer>,
    texture_2d: Option<glow::Texture>,
    shader_storage_buffer: Option<glow::Buffer>,
    shader_storage_binding_zero: Option<glow::Buffer>,
    copy_read_buffer: Option<glow::Buffer>,
    copy_write_buffer: Option<glow::Buffer>,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    clear_color: [f32; 4],
    clear_depth: f32,
    color_mask: [bool; 4],
    blend: bool,
    depth_test: bool,
    scissor_test: bool,
    program_point_size: bool,
    depth_mask: bool,
    depth_func: i32,
    blend_src_rgb: i32,
    blend_dst_rgb: i32,
    blend_src_alpha: i32,
    blend_dst_alpha: i32,
    blend_equation_rgb: i32,
    blend_equation_alpha: i32,
}

impl SavedGlState {
    pub fn capture(gl: &glow::Context) -> Self {
        let mut viewport = [0; 4];
        let mut scissor_box = [0; 4];
        let mut clear_color = [0.0; 4];
        // SAFETY: SceneRuntime holds the current GL context exclusively. All
        // queried enum names have the scalar/array shapes required by glow.
        unsafe {
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor_box);
            gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear_color);
            let indexed_storage =
                gl.get_parameter_indexed_i32(glow::SHADER_STORAGE_BUFFER_BINDING, 0);
            Self {
                program: gl.get_parameter_program(glow::CURRENT_PROGRAM),
                vertex_array: gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
                draw_framebuffer: gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                read_framebuffer: gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
                renderbuffer: gl.get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING),
                texture_2d: gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
                shader_storage_buffer: gl.get_parameter_buffer(glow::SHADER_STORAGE_BUFFER_BINDING),
                shader_storage_binding_zero: native_buffer(indexed_storage),
                copy_read_buffer: gl.get_parameter_buffer(glow::COPY_READ_BUFFER_BINDING),
                copy_write_buffer: gl.get_parameter_buffer(glow::COPY_WRITE_BUFFER_BINDING),
                viewport,
                scissor_box,
                clear_color,
                clear_depth: gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE),
                color_mask: gl.get_parameter_bool_array(glow::COLOR_WRITEMASK),
                blend: gl.is_enabled(glow::BLEND),
                depth_test: gl.is_enabled(glow::DEPTH_TEST),
                scissor_test: gl.is_enabled(glow::SCISSOR_TEST),
                program_point_size: gl.is_enabled(glow::PROGRAM_POINT_SIZE),
                depth_mask: gl.get_parameter_bool(glow::DEPTH_WRITEMASK),
                depth_func: gl.get_parameter_i32(glow::DEPTH_FUNC),
                blend_src_rgb: gl.get_parameter_i32(glow::BLEND_SRC_RGB),
                blend_dst_rgb: gl.get_parameter_i32(glow::BLEND_DST_RGB),
                blend_src_alpha: gl.get_parameter_i32(glow::BLEND_SRC_ALPHA),
                blend_dst_alpha: gl.get_parameter_i32(glow::BLEND_DST_ALPHA),
                blend_equation_rgb: gl.get_parameter_i32(glow::BLEND_EQUATION_RGB),
                blend_equation_alpha: gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA),
            }
        }
    }

    pub fn restore(self, gl: &glow::Context) {
        // SAFETY: every stored handle/value was captured from this same live
        // context immediately before SceneRuntime changed it. No captured
        // resource is owned or destroyed by SceneRuntime.
        unsafe {
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vertex_array);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_framebuffer);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_framebuffer);
            gl.bind_renderbuffer(glow::RENDERBUFFER, self.renderbuffer);
            gl.bind_texture(glow::TEXTURE_2D, self.texture_2d);
            gl.bind_buffer_base(
                glow::SHADER_STORAGE_BUFFER,
                0,
                self.shader_storage_binding_zero,
            );
            // BindBufferBase also changes the generic binding. Restore that
            // separately after the indexed slot.
            gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, self.shader_storage_buffer);
            gl.bind_buffer(glow::COPY_READ_BUFFER, self.copy_read_buffer);
            gl.bind_buffer(glow::COPY_WRITE_BUFFER, self.copy_write_buffer);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.scissor(
                self.scissor_box[0],
                self.scissor_box[1],
                self.scissor_box[2],
                self.scissor_box[3],
            );
            gl.clear_color(
                self.clear_color[0],
                self.clear_color[1],
                self.clear_color[2],
                self.clear_color[3],
            );
            gl.clear_depth_f32(self.clear_depth);
            gl.color_mask(
                self.color_mask[0],
                self.color_mask[1],
                self.color_mask[2],
                self.color_mask[3],
            );
            gl.depth_mask(self.depth_mask);
            gl.depth_func(self.depth_func as u32);
            gl.blend_func_separate(
                self.blend_src_rgb as u32,
                self.blend_dst_rgb as u32,
                self.blend_src_alpha as u32,
                self.blend_dst_alpha as u32,
            );
            gl.blend_equation_separate(
                self.blend_equation_rgb as u32,
                self.blend_equation_alpha as u32,
            );
            restore_enable(gl, glow::BLEND, self.blend);
            restore_enable(gl, glow::DEPTH_TEST, self.depth_test);
            restore_enable(gl, glow::SCISSOR_TEST, self.scissor_test);
            restore_enable(gl, glow::PROGRAM_POINT_SIZE, self.program_point_size);
        }
    }
}

fn native_buffer(value: i32) -> Option<glow::Buffer> {
    NonZeroU32::new(value as u32).map(glow::NativeBuffer)
}

fn restore_enable(gl: &glow::Context, capability: u32, enabled: bool) {
    if enabled {
        // SAFETY: `capability` is one of this module's fixed enable enums and
        // the caller holds the current context exclusively.
        unsafe { gl.enable(capability) };
    } else {
        // SAFETY: same fixed capability and current-context guarantee as the
        // enabling branch.
        unsafe { gl.disable(capability) };
    }
}
