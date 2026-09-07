//! Fixed-step Particle buffer allocation, reset, replay, and checkpoint copy.

use glow::HasContext;

use super::*;

pub(super) fn allocate_particle_buffer(
    gl: &glow::Context,
    capacity: u32,
) -> Result<glow::Buffer, LibraryError> {
    let bytes = u64::from(capacity)
        .checked_mul(PARTICLE_STRIDE_BYTES)
        .and_then(|bytes| i32::try_from(bytes).ok())
        .ok_or_else(|| LibraryError::Render("GPU Particle buffer size overflow".to_string()))?;
    // SAFETY: SceneRuntime invokes this helper only while its owning glutin
    // context is current and exclusively borrowed.
    let buffer = unsafe { gl.create_buffer() }.map_err(|error| {
        LibraryError::Render(format!(
            "Cannot create GPU Particle storage buffer: {error}"
        ))
    })?;
    // SAFETY: `buffer` is a live handle from this context, and `bytes` was
    // checked to fit the GL signed-size boundary above.
    unsafe {
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
        gl.buffer_data_size(glow::SHADER_STORAGE_BUFFER, bytes, glow::DYNAMIC_COPY);
    }
    let errors = drain_gl_errors(gl);
    if !errors.is_empty() {
        delete_particle_buffer(gl, buffer);
        return Err(LibraryError::Render(format!(
            "GPU Particle storage allocation failed (OpenGL errors {})",
            errors
                .iter()
                .map(|error| format!("0x{error:04x}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(buffer)
}

pub(super) fn delete_particle_buffer(gl: &glow::Context, buffer: glow::Buffer) {
    // SAFETY: callers transfer one live buffer owned by SceneRuntime and call
    // this exactly once while its creating context is current.
    unsafe { gl.delete_buffer(buffer) };
}

pub(super) fn reset_particles(
    gl: &glow::Context,
    pipeline: &ParticleSimulationPipeline,
    buffer: glow::Buffer,
    capacity: u32,
) -> Result<(), LibraryError> {
    // SAFETY: the pipeline and buffer are live resources owned by this
    // SceneRuntime/context; capacity matches the buffer allocation.
    unsafe {
        gl.use_program(Some(pipeline.compute_program));
        gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(buffer));
        gl.uniform_1_u32(Some(&pipeline.compute.capacity), capacity);
        gl.uniform_1_i32(Some(&pipeline.compute.reset), 1);
        gl.dispatch_compute(capacity.div_ceil(PARTICLE_WORKGROUP_SIZE), 1, 1);
        gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT | glow::BUFFER_UPDATE_BARRIER_BIT);
    }
    gl_operation_result(gl, "reset")
}

pub(super) struct ParticleSimulationRequest<'a> {
    pub buffer: glow::Buffer,
    pub capacity: u32,
    pub seed: u32,
    pub start_step: u64,
    pub step_count: u64,
    pub parameters: &'a ParticleSceneParameters,
}

pub(super) fn simulate_particles(
    gl: &glow::Context,
    pipeline: &ParticleSimulationPipeline,
    request: ParticleSimulationRequest<'_>,
) -> Result<(), LibraryError> {
    let start_step = u32::try_from(request.start_step).map_err(|_| {
        LibraryError::Render("GPU Particle time exceeds the 32-bit kernel step range".to_string())
    })?;
    let step_count = u32::try_from(request.step_count).map_err(|_| {
        LibraryError::Render("GPU Particle replay chunk exceeds kernel limits".to_string())
    })?;
    let velocity_min = vec3_f32(request.parameters.velocity_min, "minimum velocity")?;
    let velocity_max = vec3_f32(request.parameters.velocity_max, "maximum velocity")?;
    let force_uniforms = forces::ForceUniformData::new(&request.parameters.forces)?;
    let collision_uniforms = collisions::CollisionUniformData::new(&request.parameters.collisions)?;
    let emitter_position = vec3_f32(request.parameters.emitter_position, "emitter position")?;
    let emitter_size = vec3_f32(request.parameters.emitter_size, "emitter size")?;
    let emitter_shape = match request.parameters.emitter_shape {
        ParticleEmitterShape::Point => 0,
        ParticleEmitterShape::Box => 1,
        ParticleEmitterShape::Sphere => 2,
    };
    // SAFETY: request resources belong to the current SceneRuntime context;
    // validation bounds every uniform and the dispatch covers only the
    // allocated `capacity` slots (the shader guards the final workgroup).
    unsafe {
        gl.use_program(Some(pipeline.compute_program));
        gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(request.buffer));
        gl.uniform_1_u32(Some(&pipeline.compute.capacity), request.capacity);
        gl.uniform_1_i32(Some(&pipeline.compute.reset), 0);
        gl.uniform_1_u32(Some(&pipeline.compute.seed), request.seed);
        gl.uniform_1_u32(Some(&pipeline.compute.start_step), start_step);
        gl.uniform_1_u32(Some(&pipeline.compute.step_count), step_count);
        gl.uniform_1_f32(
            Some(&pipeline.compute.rate),
            request.parameters.emission_rate.into_inner(),
        );
        gl.uniform_1_f32(
            Some(&pipeline.compute.lifetime),
            request.parameters.lifetime_seconds.into_inner(),
        );
        gl.uniform_1_i32(Some(&pipeline.compute.emitter_shape), emitter_shape);
        gl.uniform_3_f32(
            Some(&pipeline.compute.emitter_position),
            emitter_position[0],
            emitter_position[1],
            emitter_position[2],
        );
        gl.uniform_1_f32(
            Some(&pipeline.compute.emitter_radius),
            request.parameters.emitter_radius.into_inner(),
        );
        gl.uniform_3_f32(
            Some(&pipeline.compute.emitter_size),
            emitter_size[0],
            emitter_size[1],
            emitter_size[2],
        );
        gl.uniform_1_i32(
            Some(&pipeline.compute.emitter_surface_only),
            i32::from(request.parameters.emitter_surface_only),
        );
        gl.uniform_3_f32(
            Some(&pipeline.compute.velocity_min),
            velocity_min[0],
            velocity_min[1],
            velocity_min[2],
        );
        gl.uniform_3_f32(
            Some(&pipeline.compute.velocity_max),
            velocity_max[0],
            velocity_max[1],
            velocity_max[2],
        );
        force_uniforms.upload(gl, &pipeline.compute.forces);
        collision_uniforms.upload(gl, &pipeline.compute.collisions);
        gl.uniform_1_f32(
            Some(&pipeline.compute.size_min),
            request.parameters.size_min.into_inner(),
        );
        gl.uniform_1_f32(
            Some(&pipeline.compute.size_max),
            request.parameters.size_max.into_inner(),
        );
        gl.dispatch_compute(request.capacity.div_ceil(PARTICLE_WORKGROUP_SIZE), 1, 1);
        gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT | glow::VERTEX_ATTRIB_ARRAY_BARRIER_BIT);
    }
    gl_operation_result(gl, "fixed-step simulation")
}

pub(super) fn copy_particle_buffer(
    gl: &glow::Context,
    source: glow::Buffer,
    destination: glow::Buffer,
    capacity: u32,
) -> Result<(), LibraryError> {
    let bytes = i32::try_from(u64::from(capacity) * PARTICLE_STRIDE_BYTES)
        .map_err(|_| LibraryError::Render("GPU Particle checkpoint size overflow".to_string()))?;
    // SAFETY: both buffers are live and allocated by this context for the
    // same capacity; `bytes` was checked above and the ranges do not overlap.
    unsafe {
        gl.bind_buffer(glow::COPY_READ_BUFFER, Some(source));
        gl.bind_buffer(glow::COPY_WRITE_BUFFER, Some(destination));
        gl.copy_buffer_sub_data(glow::COPY_READ_BUFFER, glow::COPY_WRITE_BUFFER, 0, 0, bytes);
        gl.memory_barrier(glow::BUFFER_UPDATE_BARRIER_BIT | glow::SHADER_STORAGE_BARRIER_BIT);
    }
    gl_operation_result(gl, "checkpoint copy")
}
