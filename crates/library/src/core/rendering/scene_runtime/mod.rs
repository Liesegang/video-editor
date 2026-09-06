//! Stateful GPU execution boundary shared by preview and export renderers.

mod forces;
mod gl_backend;
mod point_fields;
#[cfg(test)]
mod readback;
mod shaders;
mod simulation;

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use crate::rendering::gl_resources::SavedGlState;
use glow::HasContext;
use sha2::{Digest, Sha256};

use crate::error::LibraryError;
use crate::model::frame::particle::{
    PARTICLE_CHECKPOINT_INTERVAL_STEPS, PARTICLE_MAX_CHECKPOINTS, PARTICLE_MAX_REPLAY_STEPS,
    ParticleEmitterShape, ParticleSceneFrame, ParticleSceneParameters, SceneInvocationKey,
    particle_lifetime_steps,
};
use crate::model::property::Vec3;
use crate::rendering::renderer::Affine2D;

pub(crate) use gl_backend::SceneTextureFormat;
use gl_backend::{
    PARTICLE_STRIDE_BYTES, PARTICLE_VERTICES_PER_SPRITE, PARTICLE_WORKGROUP_SIZE, ParticlePipeline,
    SceneTarget, probe_capabilities,
};
use simulation::{
    ParticleSimulationRequest, allocate_particle_buffer, copy_particle_buffer,
    delete_particle_buffer, reset_particles, simulate_particles,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct SceneRuntimeLimits {
    pub max_live_invocations: usize,
    pub max_compiled_pipelines: usize,
    pub max_state_bytes: u64,
    pub max_target_bytes: u64,
}

impl Default for SceneRuntimeLimits {
    fn default() -> Self {
        Self {
            // A default-capacity invocation with all eight checkpoints uses
            // about 3.4 MiB. Sixty-four ordinary placements therefore remain
            // resident while the byte budget still bounds large-capacity use.
            max_live_invocations: 64,
            max_compiled_pipelines: 128,
            max_state_bytes: 512 * 1024 * 1024,
            max_target_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SceneTexture {
    pub texture_id: u32,
    pub width: u32,
    pub height: u32,
    pub format: SceneTextureFormat,
}

struct ParticleCheckpoint {
    step: u64,
    buffer: glow::Buffer,
}

struct ParticleInvocation {
    buffer: glow::Buffer,
    point_fields: Option<point_fields::PointFieldBuffers>,
    capacity: u32,
    executable_hash: [u8; 32],
    parameter_hash: u64,
    current_step: u64,
    checkpoints: VecDeque<ParticleCheckpoint>,
    last_used: u64,
}

impl ParticleInvocation {
    fn allocated_bytes(&self) -> u64 {
        let simulation =
            u64::from(self.capacity) * PARTICLE_STRIDE_BYTES * (1 + self.checkpoints.len() as u64);
        simulation.saturating_add(
            self.point_fields
                .as_ref()
                .map_or(0, point_fields::PointFieldBuffers::byte_len),
        )
    }

    fn point_layout_matches(
        &self,
        program: Option<&crate::model::point::PointRenderProgram>,
    ) -> bool {
        match (&self.point_fields, program) {
            (None, None) => true,
            (Some(buffers), Some(program)) => buffers.matches(program),
            _ => false,
        }
    }
}

/// Owns every mutable Particle buffer and every raw-GL object created beside
/// Ganesh. The caller guarantees that its glutin context is current.
pub(crate) struct SceneRuntime {
    gl: glow::Context,
    capability: Result<gl_backend::CapabilityProfile, String>,
    pipelines: HashMap<[u8; 32], ParticlePipeline>,
    invocations: HashMap<SceneInvocationKey, ParticleInvocation>,
    target: Option<SceneTarget>,
    use_tick: u64,
    limits: SceneRuntimeLimits,
}

impl SceneRuntime {
    pub(crate) fn new(gl: glow::Context) -> Self {
        Self::with_limits(gl, SceneRuntimeLimits::default())
    }

    pub(crate) fn with_limits(gl: glow::Context, limits: SceneRuntimeLimits) -> Self {
        let capability = probe_capabilities(&gl);
        Self {
            gl,
            capability,
            pipelines: HashMap::new(),
            invocations: HashMap::new(),
            target: None,
            use_tick: 0,
            limits,
        }
    }

    pub(crate) fn render_particle(
        &mut self,
        scene: &ParticleSceneFrame,
        transform: &Affine2D,
        target_width: u32,
        target_height: u32,
        format: SceneTextureFormat,
        premultiplied_color: [f32; 4],
    ) -> Result<SceneTexture, LibraryError> {
        scene.validate().map_err(LibraryError::Validation)?;
        validate_transform(transform)?;
        validate_color(premultiplied_color)?;
        let capability = self.capability.as_ref().map_err(|diagnostic| {
            LibraryError::Render(format!("GPU Particle unavailable: {diagnostic}"))
        })?;
        validate_target(
            capability,
            target_width,
            target_height,
            format,
            self.limits.max_target_bytes,
        )?;
        self.with_isolated_gl(|runtime| {
            runtime.render_particle_isolated(
                scene,
                transform,
                target_width,
                target_height,
                format,
                premultiplied_color,
            )
        })
    }

    /// Compile and execute the real compute/SSBO/render/FBO boundary without
    /// creating authored simulation state. Export uses this before opening an
    /// encoder so a late Particle clip cannot reveal unsupported hardware
    /// after earlier frames were already written.
    pub(crate) fn preflight_particle(
        &mut self,
        target_width: u32,
        target_height: u32,
        format: SceneTextureFormat,
    ) -> Result<SceneTexture, LibraryError> {
        let capability = self.capability.as_ref().map_err(|diagnostic| {
            LibraryError::Render(format!("GPU Particle unavailable: {diagnostic}"))
        })?;
        validate_target(
            capability,
            target_width,
            target_height,
            format,
            self.limits.max_target_bytes,
        )?;
        self.with_isolated_gl(|runtime| {
            runtime.preflight_particle_isolated(target_width, target_height, format)
        })
    }

    fn with_isolated_gl<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        // Skia has already been flushed by the caller. Restore every GL state
        // touched here even when allocation, replay, or shader compilation
        // fails; the caller resets Ganesh's state cache afterwards.
        let previous_target = self.target.as_ref().map(SceneTarget::bindings);
        let mut saved_state = SavedGlState::capture(&self.gl);
        drain_gl_errors(&self.gl);
        let result = operation(self);
        if let Some(previous_target) = previous_target
            && self
                .target
                .as_ref()
                .is_none_or(|target| target.bindings().texture_id != previous_target.texture_id)
        {
            saved_state.invalidate_destroyed_target(previous_target);
        }
        saved_state.restore(&self.gl);
        result
    }

    fn preflight_particle_isolated(
        &mut self,
        target_width: u32,
        target_height: u32,
        format: SceneTextureFormat,
    ) -> Result<SceneTexture, LibraryError> {
        self.use_tick = self.use_tick.wrapping_add(1);
        let use_tick = self.use_tick;
        // Preflight has no authored executable identity. Keep its throwaway
        // program outside the executable cache so a legitimate all-zero hash
        // cannot collide with this synthetic probe.
        let pipeline = ParticlePipeline::create(&self.gl, use_tick, None)?;
        self.ensure_target(target_width, target_height, format)?;
        let buffer = match allocate_particle_buffer(&self.gl, 1) {
            Ok(buffer) => buffer,
            Err(error) => {
                pipeline.destroy(&self.gl);
                return Err(error);
            }
        };
        let result = (|| {
            reset_particles(&self.gl, &pipeline, buffer, 1)?;
            let invocation = ParticleInvocation {
                buffer,
                point_fields: None,
                capacity: 1,
                executable_hash: [0; 32],
                parameter_hash: 0,
                current_step: 0,
                checkpoints: VecDeque::new(),
                last_used: use_tick,
            };
            let target = self.target.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Particle preflight target disappeared".to_string())
            })?;
            draw_particles(
                &self.gl,
                &pipeline,
                ParticleDrawRequest {
                    invocation: &invocation,
                    target,
                    transform: &Affine2D::IDENTITY,
                    logical_size: (target_width, target_height),
                    premultiplied_color: [0.0; 4],
                },
            )?;
            Ok(SceneTexture {
                texture_id: target.texture_id(),
                width: target.width,
                height: target.height,
                format: target.format,
            })
        })();
        delete_particle_buffer(&self.gl, buffer);
        pipeline.destroy(&self.gl);
        result
    }

    fn render_particle_isolated(
        &mut self,
        scene: &ParticleSceneFrame,
        transform: &Affine2D,
        target_width: u32,
        target_height: u32,
        format: SceneTextureFormat,
        premultiplied_color: [f32; 4],
    ) -> Result<SceneTexture, LibraryError> {
        self.use_tick = self.use_tick.wrapping_add(1);
        let use_tick = self.use_tick;
        let pipeline = self.pipeline(
            scene.executable_hash,
            scene.point_program.as_ref(),
            use_tick,
        )?;
        self.ensure_target(target_width, target_height, format)?;

        let parameter_hash = stable_parameter_hash(&scene.parameters);
        let mut invocation = match self.invocations.remove(&scene.invocation) {
            Some(invocation)
                if invocation.capacity == scene.parameters.capacity
                    && invocation.executable_hash == scene.executable_hash
                    && invocation.parameter_hash == parameter_hash
                    && invocation.point_layout_matches(scene.point_program.as_ref()) =>
            {
                invocation
            }
            Some(invocation) => {
                self.destroy_invocation(invocation);
                self.reserve_invocation(scene.parameters.capacity, scene.point_program.as_ref())?;
                self.create_invocation(scene, parameter_hash, &pipeline, use_tick)?
            }
            None => {
                self.reserve_invocation(scene.parameters.capacity, scene.point_program.as_ref())?;
                self.create_invocation(scene, parameter_hash, &pipeline, use_tick)?
            }
        };

        if let Err(error) = self.seek_invocation(&mut invocation, scene, &pipeline) {
            // Compute/reset/copy errors can leave the SSBO partially updated
            // without advancing `current_step`. Discard derived state so a
            // retry starts from a known cold buffer rather than compounding
            // the failed step.
            self.destroy_invocation(invocation);
            return Err(error);
        }
        if let (Some(program), Some(buffers), Some(point_pipeline)) = (
            scene.point_program.as_ref(),
            invocation.point_fields.as_ref(),
            pipeline.point_fields.as_ref(),
        ) && let Err(error) = point_pipeline.evaluate(
            &self.gl,
            program,
            buffers,
            invocation.buffer,
            invocation_seed(scene),
        ) {
            self.destroy_invocation(invocation);
            return Err(error);
        }
        let evaluation = (|| {
            let target = self.target.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Particle target disappeared before draw".to_string())
            })?;
            draw_particles(
                &self.gl,
                &pipeline,
                ParticleDrawRequest {
                    invocation: &invocation,
                    target,
                    transform,
                    logical_size: (scene.logical_width, scene.logical_height),
                    premultiplied_color,
                },
            )?;
            Ok(SceneTexture {
                texture_id: target.texture_id(),
                width: target.width,
                height: target.height,
                format: target.format,
            })
        })();
        invocation.last_used = use_tick;
        self.invocations
            .insert(scene.invocation.clone(), invocation);
        evaluation
    }

    fn pipeline(
        &mut self,
        executable_hash: [u8; 32],
        point_program: Option<&crate::model::point::PointRenderProgram>,
        use_tick: u64,
    ) -> Result<ParticlePipeline, LibraryError> {
        if let Some(pipeline) = self.pipelines.get_mut(&executable_hash) {
            if pipeline
                .point_fields
                .as_ref()
                .map(|fields| fields.source_hash)
                != point_fields::source_hash(point_program)?
            {
                return Err(LibraryError::Validation(
                    "Particle executable hash reused for a different Point program shape"
                        .to_string(),
                ));
            }
            pipeline.last_used = use_tick;
            return Ok(pipeline.clone());
        }
        if self.pipelines.len() >= self.limits.max_compiled_pipelines.max(1)
            && let Some(eviction_key) = self
                .pipelines
                .iter()
                .min_by_key(|(_, pipeline)| pipeline.last_used)
                .map(|(key, _)| *key)
            && let Some(pipeline) = self.pipelines.remove(&eviction_key)
        {
            pipeline.destroy(&self.gl);
        }
        let pipeline = ParticlePipeline::create(&self.gl, use_tick, point_program)?;
        self.pipelines.insert(executable_hash, pipeline.clone());
        Ok(pipeline)
    }

    fn ensure_target(
        &mut self,
        width: u32,
        height: u32,
        format: SceneTextureFormat,
    ) -> Result<(), LibraryError> {
        let reusable = self.target.as_ref().is_some_and(|target| {
            target.width == width && target.height == height && target.format == format
        });
        if reusable {
            return Ok(());
        }
        // Allocate first so a failed resize preserves the prior valid target.
        // `with_isolated_gl` also prevents a Ganesh binding to the retired
        // texture/framebuffer from being restored after destruction.
        let replacement = SceneTarget::create(&self.gl, width, height, format)?;
        if let Some(target) = self.target.replace(replacement) {
            target.destroy(&self.gl);
        }
        Ok(())
    }

    fn create_invocation(
        &self,
        scene: &ParticleSceneFrame,
        parameter_hash: u64,
        pipeline: &ParticlePipeline,
        last_used: u64,
    ) -> Result<ParticleInvocation, LibraryError> {
        let buffer = allocate_particle_buffer(&self.gl, scene.parameters.capacity)?;
        if let Err(error) = reset_particles(&self.gl, pipeline, buffer, scene.parameters.capacity) {
            delete_particle_buffer(&self.gl, buffer);
            return Err(error);
        }
        let point_fields = match scene.point_program.as_ref().map(|program| {
            point_fields::PointFieldBuffers::create(&self.gl, program, scene.parameters.capacity)
        }) {
            Some(Ok(buffers)) => Some(buffers),
            Some(Err(error)) => {
                delete_particle_buffer(&self.gl, buffer);
                return Err(error);
            }
            None => None,
        };
        Ok(ParticleInvocation {
            buffer,
            point_fields,
            capacity: scene.parameters.capacity,
            executable_hash: scene.executable_hash,
            parameter_hash,
            current_step: 0,
            checkpoints: VecDeque::new(),
            last_used,
        })
    }

    fn seek_invocation(
        &self,
        invocation: &mut ParticleInvocation,
        scene: &ParticleSceneFrame,
        pipeline: &ParticlePipeline,
    ) -> Result<(), LibraryError> {
        if scene.target_step < invocation.current_step {
            if let Some(checkpoint) = invocation
                .checkpoints
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.step <= scene.target_step)
            {
                copy_particle_buffer(
                    &self.gl,
                    checkpoint.buffer,
                    invocation.buffer,
                    invocation.capacity,
                )?;
                invocation.current_step = checkpoint.step;
            } else {
                reset_particles(&self.gl, pipeline, invocation.buffer, invocation.capacity)?;
                invocation.current_step = 0;
            }
        }
        if scene.target_step.saturating_sub(invocation.current_step) > PARTICLE_MAX_REPLAY_STEPS {
            // The executable slice has no persistent emitter state: a live
            // particle depends only on emissions within its maximum lifetime.
            // Reconstruct that bounded suffix with absolute step numbers so a
            // cold start or distant seek does not replay the entire Clip.
            reset_particles(&self.gl, pipeline, invocation.buffer, invocation.capacity)?;
            invocation.current_step = bounded_replay_origin(scene);
        }
        validate_replay(invocation.current_step, scene.target_step)?;
        while invocation.current_step < scene.target_step {
            let until_checkpoint = PARTICLE_CHECKPOINT_INTERVAL_STEPS
                - invocation.current_step % PARTICLE_CHECKPOINT_INTERVAL_STEPS;
            let count = (scene.target_step - invocation.current_step).min(until_checkpoint);
            simulate_particles(
                &self.gl,
                pipeline,
                ParticleSimulationRequest {
                    buffer: invocation.buffer,
                    capacity: invocation.capacity,
                    seed: invocation_seed(scene),
                    start_step: invocation.current_step,
                    step_count: count,
                    parameters: &scene.parameters,
                },
            )?;
            invocation.current_step += count;
            if invocation
                .current_step
                .is_multiple_of(PARTICLE_CHECKPOINT_INTERVAL_STEPS)
            {
                self.store_checkpoint(invocation)?;
            }
        }
        Ok(())
    }

    fn store_checkpoint(&self, invocation: &mut ParticleInvocation) -> Result<(), LibraryError> {
        while invocation.checkpoints.len() >= PARTICLE_MAX_CHECKPOINTS {
            if let Some(checkpoint) = invocation.checkpoints.pop_front() {
                delete_particle_buffer(&self.gl, checkpoint.buffer);
            }
        }
        let checkpoint_bytes = u64::from(invocation.capacity) * PARTICLE_STRIDE_BYTES;
        let resident_bytes = self
            .invocations
            .values()
            .map(ParticleInvocation::allocated_bytes)
            .sum::<u64>()
            .saturating_add(invocation.allocated_bytes());
        if resident_bytes.saturating_add(checkpoint_bytes) > self.limits.max_state_bytes {
            // Checkpoints are derived cache data. Skipping one preserves exact
            // forward simulation while respecting the hard memory budget.
            return Ok(());
        }
        let buffer = allocate_particle_buffer(&self.gl, invocation.capacity)?;
        if let Err(error) =
            copy_particle_buffer(&self.gl, invocation.buffer, buffer, invocation.capacity)
        {
            delete_particle_buffer(&self.gl, buffer);
            return Err(error);
        }
        invocation.checkpoints.push_back(ParticleCheckpoint {
            step: invocation.current_step,
            buffer,
        });
        Ok(())
    }

    fn reserve_invocation(
        &mut self,
        capacity: u32,
        point_program: Option<&crate::model::point::PointRenderProgram>,
    ) -> Result<(), LibraryError> {
        let required_bytes = point_fields::required_invocation_bytes(point_program, capacity)?;
        if required_bytes > self.limits.max_state_bytes {
            return Err(LibraryError::Render(format!(
                "GPU Particle invocation requires {required_bytes} bytes, exceeding the configured {}-byte state budget",
                self.limits.max_state_bytes
            )));
        }
        while self.invocations.len() >= self.limits.max_live_invocations.max(1)
            || self.resident_state_bytes().saturating_add(required_bytes)
                > self.limits.max_state_bytes
        {
            let Some(key) = self
                .invocations
                .iter()
                .min_by_key(|(_, invocation)| invocation.last_used)
                .map(|(key, _)| key.clone())
            else {
                return Err(LibraryError::Render(
                    "GPU Particle state budget cannot admit a new invocation".to_string(),
                ));
            };
            if let Some(invocation) = self.invocations.remove(&key) {
                self.destroy_invocation(invocation);
            }
        }
        Ok(())
    }

    fn resident_state_bytes(&self) -> u64 {
        self.invocations
            .values()
            .map(ParticleInvocation::allocated_bytes)
            .sum()
    }

    fn destroy_invocation(&self, invocation: ParticleInvocation) {
        delete_particle_buffer(&self.gl, invocation.buffer);
        if let Some(point_fields) = invocation.point_fields {
            point_fields.destroy(&self.gl);
        }
        for checkpoint in invocation.checkpoints {
            delete_particle_buffer(&self.gl, checkpoint.buffer);
        }
    }
}

impl Drop for SceneRuntime {
    fn drop(&mut self) {
        for (_, invocation) in self.invocations.drain() {
            delete_particle_buffer(&self.gl, invocation.buffer);
            if let Some(point_fields) = invocation.point_fields {
                point_fields.destroy(&self.gl);
            }
            for checkpoint in invocation.checkpoints {
                delete_particle_buffer(&self.gl, checkpoint.buffer);
            }
        }
        for (_, pipeline) in self.pipelines.drain() {
            pipeline.destroy(&self.gl);
        }
        if let Some(target) = self.target.take() {
            target.destroy(&self.gl);
        }
    }
}

fn validate_transform(transform: &Affine2D) -> Result<(), LibraryError> {
    let values = [
        transform.scale_x,
        transform.skew_x,
        transform.translate_x,
        transform.skew_y,
        transform.scale_y,
        transform.translate_y,
    ];
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(())
        .ok_or_else(|| {
            LibraryError::Validation("GPU Particle transform must be finite".to_string())
        })
}

fn validate_color(color: [f32; 4]) -> Result<(), LibraryError> {
    color
        .iter()
        .all(|value| value.is_finite())
        .then_some(())
        .ok_or_else(|| {
            LibraryError::Render("GPU Particle working color must be finite".to_string())
        })
}

fn validate_target(
    capability: &gl_backend::CapabilityProfile,
    width: u32,
    height: u32,
    format: SceneTextureFormat,
    max_target_bytes: u64,
) -> Result<(), LibraryError> {
    if width == 0 || height == 0 {
        return Err(LibraryError::Render(
            "GPU Particle target dimensions must be positive".to_string(),
        ));
    }
    if width > capability.max_texture_size || height > capability.max_texture_size {
        return Err(LibraryError::Render(format!(
            "GPU Particle target {width}x{height} exceeds {} maximum texture size {}",
            capability.label, capability.max_texture_size
        )));
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(format.bytes_per_pixel()))
        .ok_or_else(|| LibraryError::Render("GPU Particle target size overflow".to_string()))?;
    if bytes > max_target_bytes {
        return Err(LibraryError::Render(format!(
            "GPU Particle target requires {bytes} bytes, exceeding the {max_target_bytes}-byte scene target limit"
        )));
    }
    Ok(())
}

fn stable_parameter_hash(parameters: &ParticleSceneParameters) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Sprite color is a render-only uniform. It must not invalidate or replay
    // simulation history when the user grades/animates appearance.
    parameters.capacity.hash(&mut hasher);
    parameters.emission_rate.hash(&mut hasher);
    parameters.lifetime_seconds.hash(&mut hasher);
    parameters.seed.hash(&mut hasher);
    parameters.emitter_shape.hash(&mut hasher);
    parameters.emitter_position.hash(&mut hasher);
    parameters.emitter_radius.hash(&mut hasher);
    parameters.emitter_size.hash(&mut hasher);
    parameters.emitter_surface_only.hash(&mut hasher);
    parameters.velocity_min.hash(&mut hasher);
    parameters.velocity_max.hash(&mut hasher);
    parameters.forces.hash(&mut hasher);
    parameters.size_min.hash(&mut hasher);
    parameters.size_max.hash(&mut hasher);
    hasher.finish()
}

fn validate_replay(current_step: u64, target_step: u64) -> Result<u64, LibraryError> {
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

fn bounded_replay_origin(scene: &ParticleSceneFrame) -> u64 {
    let lifetime_steps =
        particle_lifetime_steps(f64::from(scene.parameters.lifetime_seconds.into_inner()));
    scene
        .target_step
        .saturating_sub(lifetime_steps.clamp(1, PARTICLE_MAX_REPLAY_STEPS))
}

pub(crate) fn invocation_seed(scene: &ParticleSceneFrame) -> u32 {
    let mut digest = Sha256::new();
    digest.update(scene.parameters.seed.to_le_bytes());
    digest.update(scene.invocation.module_instance_id.as_uuid().as_bytes());
    digest.update(scene.random_stream_id.as_bytes());
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

struct ParticleDrawRequest<'a> {
    invocation: &'a ParticleInvocation,
    target: &'a SceneTarget,
    transform: &'a Affine2D,
    logical_size: (u32, u32),
    premultiplied_color: [f32; 4],
}

fn draw_particles(
    gl: &glow::Context,
    pipeline: &ParticlePipeline,
    request: ParticleDrawRequest<'_>,
) -> Result<(), LibraryError> {
    let determinant = request.transform.scale_x * request.transform.scale_y
        - request.transform.skew_x * request.transform.skew_y;
    // SAFETY: the pipeline, invocation buffer, and target all belong to this
    // current context. Scene validation guarantees finite uniforms and target
    // allocation; the draw count never exceeds the SSBO capacity.
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
            // A singular affine has zero visible area for every other layer.
            // OpenGL clamps rasterized point size to an implementation-defined
            // minimum (usually one pixel), so issuing a draw here would turn a
            // hidden Particle clip into a bright line or point.
            gl.memory_barrier(glow::FRAMEBUFFER_BARRIER_BIT | glow::TEXTURE_FETCH_BARRIER_BIT);
            return gl_operation_result(gl, "singular Particle clear");
        }
        gl.enable(glow::BLEND);
        gl.blend_equation(glow::FUNC_ADD);
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        // Soft/translucent sprites use deterministic SSBO slot order. The
        // first executable slice is 2D and deliberately allocates no depth
        // attachment; a later 3D/OIT pass gets its own explicit budget.
        gl.use_program(Some(pipeline.render_program));
        gl.bind_vertex_array(Some(pipeline.vertex_array));
        gl.bind_buffer_base(
            glow::SHADER_STORAGE_BUFFER,
            0,
            Some(request.invocation.buffer),
        );
        if let Some(point_fields) = &request.invocation.point_fields {
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(point_fields.colors));
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
            .invocation
            .capacity
            .checked_mul(PARTICLE_VERTICES_PER_SPRITE)
            .and_then(|count| i32::try_from(count).ok())
            .ok_or_else(|| LibraryError::Render("GPU Particle draw count overflow".to_string()))?;
        gl.draw_arrays(glow::TRIANGLES, 0, vertex_count);
        gl.memory_barrier(glow::FRAMEBUFFER_BARRIER_BIT | glow::TEXTURE_FETCH_BARRIER_BIT);
    }
    gl_operation_result(gl, "sprite render")
}

fn vec3_f32(value: Vec3, label: &str) -> Result<[f32; 3], LibraryError> {
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
            LibraryError::Validation(format!("GPU Particle {label} must fit finite GPU floats"))
        })
}

fn gl_operation_result(gl: &glow::Context, operation: &str) -> Result<(), LibraryError> {
    let errors = drain_gl_errors(gl);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LibraryError::Render(format!(
            "GPU Particle {operation} failed (OpenGL errors {})",
            errors
                .iter()
                .map(|error| format!("0x{error:04x}"))
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

fn drain_gl_errors(gl: &glow::Context) -> Vec<u32> {
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

#[cfg(test)]
mod tests;
