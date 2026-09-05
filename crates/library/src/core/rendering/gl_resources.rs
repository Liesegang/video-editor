//! Raw OpenGL program construction and state isolation shared by renderer stages.

use crate::error::LibraryError;
use glow::HasContext;
use std::num::NonZeroU32;

#[derive(Clone, Copy)]
pub(crate) struct GlTargetBindings {
    pub texture_id: u32,
    pub framebuffer_id: u32,
}

pub(crate) fn link_program(
    gl: &glow::Context,
    stages: &[(u32, &str)],
    label: &str,
) -> Result<glow::Program, LibraryError> {
    // SAFETY: renderer calls pipeline construction with its current and
    // exclusively borrowed OpenGL context.
    let program = unsafe { gl.create_program() }.map_err(|error| {
        LibraryError::Render(format!("Cannot create GPU {label} program: {error}"))
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
                    "Cannot create GPU {label} shader: {error}"
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
                "GPU {label} shader compilation failed: {log}"
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
            "GPU {label} program link failed: {log}"
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

/// Exact subset of OpenGL state touched by renderer. The caller flushes
/// Ganesh first and resets its state cache after this snapshot is restored.
pub(crate) struct SavedGlState {
    program: Option<glow::Program>,
    vertex_array: Option<glow::VertexArray>,
    draw_framebuffer: Option<glow::Framebuffer>,
    read_framebuffer: Option<glow::Framebuffer>,
    texture_2d: Option<glow::Texture>,
    sampler: Option<glow::Sampler>,
    texture_unit: u32,
    shader_storage_buffer: Option<glow::Buffer>,
    shader_storage_binding_zero: Option<glow::Buffer>,
    copy_read_buffer: Option<glow::Buffer>,
    copy_write_buffer: Option<glow::Buffer>,
    pixel_unpack_buffer: Option<glow::Buffer>,
    unpack_alignment: i32,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    clear_color: [f32; 4],
    color_mask: [bool; 4],
    blend: bool,
    depth_test: bool,
    scissor_test: bool,
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
        // SAFETY: renderer holds the current GL context exclusively. All
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
                texture_2d: gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
                sampler: gl.get_parameter_sampler(glow::SAMPLER_BINDING),
                texture_unit: gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32 - glow::TEXTURE0,
                shader_storage_buffer: gl.get_parameter_buffer(glow::SHADER_STORAGE_BUFFER_BINDING),
                shader_storage_binding_zero: native_buffer(indexed_storage),
                copy_read_buffer: gl.get_parameter_buffer(glow::COPY_READ_BUFFER_BINDING),
                copy_write_buffer: gl.get_parameter_buffer(glow::COPY_WRITE_BUFFER_BINDING),
                pixel_unpack_buffer: gl.get_parameter_buffer(glow::PIXEL_UNPACK_BUFFER_BINDING),
                unpack_alignment: gl.get_parameter_i32(glow::UNPACK_ALIGNMENT),
                viewport,
                scissor_box,
                clear_color,
                color_mask: gl.get_parameter_bool_array(glow::COLOR_WRITEMASK),
                blend: gl.is_enabled(glow::BLEND),
                depth_test: gl.is_enabled(glow::DEPTH_TEST),
                scissor_test: gl.is_enabled(glow::SCISSOR_TEST),
                blend_src_rgb: gl.get_parameter_i32(glow::BLEND_SRC_RGB),
                blend_dst_rgb: gl.get_parameter_i32(glow::BLEND_DST_RGB),
                blend_src_alpha: gl.get_parameter_i32(glow::BLEND_SRC_ALPHA),
                blend_dst_alpha: gl.get_parameter_i32(glow::BLEND_DST_ALPHA),
                blend_equation_rgb: gl.get_parameter_i32(glow::BLEND_EQUATION_RGB),
                blend_equation_alpha: gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA),
            }
        }
    }

    /// Scene target replacement may delete a texture/framebuffer which
    /// Ganesh left bound after sampling. Such names cannot be restored; bind
    /// the default object instead and let the caller reset Ganesh's cache.
    pub fn invalidate_destroyed_target(&mut self, target: GlTargetBindings) {
        if self
            .texture_2d
            .is_some_and(|texture| texture.0.get() == target.texture_id)
        {
            self.texture_2d = None;
        }
        if self
            .draw_framebuffer
            .is_some_and(|framebuffer| framebuffer.0.get() == target.framebuffer_id)
        {
            self.draw_framebuffer = None;
        }
        if self
            .read_framebuffer
            .is_some_and(|framebuffer| framebuffer.0.get() == target.framebuffer_id)
        {
            self.read_framebuffer = None;
        }
    }

    pub fn restore(self, gl: &glow::Context) {
        // SAFETY: every stored handle/value was captured from this same live
        // context immediately before renderer changed it. No captured
        // resource is owned or destroyed by renderer.
        unsafe {
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vertex_array);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_framebuffer);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_framebuffer);
            gl.bind_texture(glow::TEXTURE_2D, self.texture_2d);
            gl.bind_sampler(self.texture_unit, self.sampler);
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
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, self.pixel_unpack_buffer);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, self.unpack_alignment);
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
            gl.color_mask(
                self.color_mask[0],
                self.color_mask[1],
                self.color_mask[2],
                self.color_mask[3],
            );
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
