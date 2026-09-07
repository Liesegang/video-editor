//! Shared Point validation, deterministic identity, and Sprite drawing.

use glow::HasContext;
use sha2::{Digest, Sha256};

use super::*;
use crate::model::property::Vec3;

pub(super) fn validate_transform(transform: &Affine2D) -> Result<(), LibraryError> {
    [
        transform.scale_x,
        transform.skew_x,
        transform.translate_x,
        transform.skew_y,
        transform.scale_y,
        transform.translate_y,
    ]
    .iter()
    .all(|value| value.is_finite())
    .then_some(())
    .ok_or_else(|| LibraryError::Validation("GPU Point transform must be finite".to_string()))
}

pub(super) fn validate_color(color: [f32; 4]) -> Result<(), LibraryError> {
    color
        .iter()
        .all(|value| value.is_finite())
        .then_some(())
        .ok_or_else(|| LibraryError::Render("GPU Point working color must be finite".to_string()))
}

pub(super) fn validate_target(
    capability: &gl_backend::CapabilityProfile,
    width: u32,
    height: u32,
    format: SceneTextureFormat,
    max_target_bytes: u64,
) -> Result<(), LibraryError> {
    if width == 0 || height == 0 {
        return Err(LibraryError::Render(
            "GPU Point target dimensions must be positive".to_string(),
        ));
    }
    if width > capability.max_texture_size || height > capability.max_texture_size {
        return Err(LibraryError::Render(format!(
            "GPU Point target {width}x{height} exceeds {} maximum texture size {}",
            capability.label, capability.max_texture_size
        )));
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(format.bytes_per_pixel()))
        .ok_or_else(|| LibraryError::Render("GPU Point target size overflow".to_string()))?;
    if bytes > max_target_bytes {
        return Err(LibraryError::Render(format!(
            "GPU Point target requires {bytes} bytes, exceeding the {max_target_bytes}-byte scene target limit"
        )));
    }
    Ok(())
}

pub(super) fn validate_replay(current_step: u64, target_step: u64) -> Result<u64, LibraryError> {
    let replay_steps = target_step.checked_sub(current_step).ok_or_else(|| {
        LibraryError::Render("GPU Particle replay origin is after its target".to_string())
    })?;
    if replay_steps > PARTICLE_MAX_REPLAY_STEPS {
        return Err(LibraryError::Render(format!(
            "GPU Particle seek requires {replay_steps} fixed steps, exceeding the per-request limit {PARTICLE_MAX_REPLAY_STEPS}; seek nearer or render sequentially"
        )));
    }
    Ok(replay_steps)
}

pub(super) fn bounded_replay_origin(parameters: &ParticleSceneParameters, target_step: u64) -> u64 {
    let lifetime_steps =
        particle_lifetime_steps(f64::from(parameters.lifetime_seconds.into_inner()));
    target_step.saturating_sub(lifetime_steps.clamp(1, PARTICLE_MAX_REPLAY_STEPS))
}

pub(crate) fn invocation_seed(scene: &PointSceneFrame) -> u32 {
    let mut digest = Sha256::new();
    digest.update(scene.source.seed().to_le_bytes());
    digest.update(scene.invocation.module_instance_id.as_uuid().as_bytes());
    digest.update(scene.source_node_id.as_bytes());
    digest.update(
        scene
            .invocation
            .instance_path
            .root_timeline_id
            .as_uuid()
            .as_bytes(),
    );
    for segment in &scene.invocation.instance_path.composition_items {
        digest.update(segment.as_uuid().as_bytes());
    }
    let digest = digest.finalize();
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}

pub(super) fn point_source_binding<'a>(
    invocation: &'a PointInvocation,
    source: &'a PointSceneSource,
) -> Result<PointSourceBinding<'a>, LibraryError> {
    match (&invocation.particle, source) {
        (Some(particle), PointSceneSource::Particle { .. }) => Ok(PointSourceBinding::Particle {
            buffer: particle.buffer,
        }),
        (None, PointSceneSource::Grid(parameters)) => Ok(PointSourceBinding::Grid(parameters)),
        _ => Err(LibraryError::Validation(
            "Point source changed without rebuilding its invocation state".to_string(),
        )),
    }
}

pub(super) struct PointDrawRequest<'a> {
    pub capacity: u32,
    pub point_fields: Option<&'a point_fields::PointFieldBuffers>,
    pub point_source: &'a PointSourceBinding<'a>,
    pub target: &'a SceneTarget,
    pub transform: &'a Affine2D,
    pub logical_size: (u32, u32),
    pub premultiplied_color: [f32; 4],
}

pub(super) fn draw_points(
    gl: &glow::Context,
    pipeline: &PointPipeline,
    request: PointDrawRequest<'_>,
) -> Result<(), LibraryError> {
    let determinant = request.transform.scale_x * request.transform.scale_y
        - request.transform.skew_x * request.transform.skew_y;
    // SAFETY: the pipeline, source resources, and target all belong to this
    // current context. Validation bounds uniforms and the draw count.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(request.target.framebuffer));
        gl.viewport(
            0,
            0,
            request.target.width as i32,
            request.target.height as i32,
        );
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::DEPTH_TEST);
        gl.color_mask(true, true, true, true);
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
        if determinant.abs() <= f64::EPSILON {
            gl.memory_barrier(glow::FRAMEBUFFER_BARRIER_BIT | glow::TEXTURE_FETCH_BARRIER_BIT);
            return gl_operation_result(gl, "singular Point clear");
        }
        gl.enable(glow::BLEND);
        gl.blend_equation(glow::FUNC_ADD);
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        gl.use_program(Some(pipeline.render_program));
        gl.bind_vertex_array(Some(pipeline.vertex_array));
        request.point_source.bind(gl, &pipeline.source)?;
        if let Some(point_fields) = request.point_fields {
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(point_fields.colors));
            if let Some(positions) = point_fields.positions {
                gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 4, Some(positions));
            }
        }
        gl.uniform_2_f32(
            Some(&pipeline.render.logical_size),
            request.logical_size.0 as f32,
            request.logical_size.1 as f32,
        );
        gl.uniform_2_f32(
            Some(&pipeline.render.target_size),
            request.target.width as f32,
            request.target.height as f32,
        );
        gl.uniform_3_f32(
            Some(&pipeline.render.affine_x),
            request.transform.scale_x as f32,
            request.transform.skew_x as f32,
            request.transform.translate_x as f32,
        );
        gl.uniform_3_f32(
            Some(&pipeline.render.affine_y),
            request.transform.skew_y as f32,
            request.transform.scale_y as f32,
            request.transform.translate_y as f32,
        );
        gl.uniform_1_f32(
            Some(&pipeline.render.focal_length),
            request.logical_size.1.max(1) as f32,
        );
        if let Some(location) = &pipeline.render.premultiplied_color {
            gl.uniform_4_f32(
                Some(location),
                request.premultiplied_color[0],
                request.premultiplied_color[1],
                request.premultiplied_color[2],
                request.premultiplied_color[3],
            );
        }
        if let Some(location) = &pipeline.render.output_srgba {
            gl.uniform_1_i32(
                Some(location),
                i32::from(request.target.format == SceneTextureFormat::Srgba8),
            );
        }
        let vertex_count = request
            .capacity
            .checked_mul(PARTICLE_VERTICES_PER_SPRITE)
            .and_then(|count| i32::try_from(count).ok())
            .ok_or_else(|| LibraryError::Render("GPU Point draw count overflow".to_string()))?;
        gl.draw_arrays(glow::TRIANGLES, 0, vertex_count);
        gl.memory_barrier(glow::FRAMEBUFFER_BARRIER_BIT | glow::TEXTURE_FETCH_BARRIER_BIT);
    }
    gl_operation_result(gl, "sprite render")
}

pub(super) fn vec3_f32(value: Vec3, label: &str) -> Result<[f32; 3], LibraryError> {
    let converted = [
        value.x.into_inner() as f32,
        value.y.into_inner() as f32,
        value.z.into_inner() as f32,
    ];
    converted
        .iter()
        .all(|component| component.is_finite())
        .then_some(converted)
        .ok_or_else(|| {
            LibraryError::Validation(format!("GPU Point {label} must fit finite GPU floats"))
        })
}

pub(super) fn gl_operation_result(gl: &glow::Context, operation: &str) -> Result<(), LibraryError> {
    let errors = drain_gl_errors(gl);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LibraryError::Render(format!(
            "GPU Point {operation} failed (OpenGL errors {})",
            errors
                .iter()
                .map(|error| format!("0x{error:04x}"))
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

pub(super) fn drain_gl_errors(gl: &glow::Context) -> Vec<u32> {
    let mut errors = Vec::new();
    loop {
        // SAFETY: every caller holds SceneRuntime's current GL context
        // exclusively; querying the error flag does not access user memory.
        let error = unsafe { gl.get_error() };
        if error == glow::NO_ERROR {
            break;
        }
        errors.push(error);
        if errors.len() == 16 {
            break;
        }
    }
    errors
}
