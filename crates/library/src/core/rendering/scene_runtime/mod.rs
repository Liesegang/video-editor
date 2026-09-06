//! Stateful GPU execution boundary shared by preview and export renderers.

mod forces;
mod gl_backend;
mod point_fields;
#[cfg(test)]
mod readback;
#[cfg(test)]
pub(crate) use readback::PointFieldReadback;
mod render;
mod shaders;
mod simulation;
mod source;

use crate::rendering::gl_resources::SavedGlState;
use std::collections::{HashMap, VecDeque};

use crate::error::LibraryError;
use crate::model::frame::particle::{
    PARTICLE_CHECKPOINT_INTERVAL_STEPS, PARTICLE_MAX_CHECKPOINTS, PARTICLE_MAX_REPLAY_STEPS,
    ParticleEmitterShape, ParticleSceneParameters, particle_lifetime_steps,
};
use crate::model::frame::point::{PointSceneFrame, PointSceneSource, SceneInvocationKey};
use crate::rendering::renderer::Affine2D;

pub(crate) use gl_backend::SceneTextureFormat;
use gl_backend::{
    PARTICLE_STRIDE_BYTES, PARTICLE_VERTICES_PER_SPRITE, PARTICLE_WORKGROUP_SIZE,
    ParticleSimulationPipeline, PointPipeline, SceneTarget, probe_capabilities,
};
pub(crate) use render::invocation_seed;
use render::{
    PointDrawRequest, bounded_replay_origin, drain_gl_errors, draw_points, gl_operation_result,
    point_source_binding, stable_parameter_hash, validate_color, validate_replay, validate_target,
    validate_transform, vec3_f32,
};
use simulation::{
    ParticleSimulationRequest, allocate_particle_buffer, copy_particle_buffer,
    delete_particle_buffer, reset_particles, simulate_particles,
};
use source::{PointSourceBinding, PointSourceKind};

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

struct ParticleState {
    buffer: glow::Buffer,
    parameter_hash: u64,
    current_step: u64,
    checkpoints: VecDeque<ParticleCheckpoint>,
}

struct PointInvocation {
    particle: Option<ParticleState>,
    point_fields: Option<point_fields::PointFieldBuffers>,
    capacity: u32,
    executable_hash: [u8; 32],
    last_used: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PointPipelineKey {
    source_kind: PointSourceKind,
    field_source_hash: Option<[u8; 32]>,
}

impl PointInvocation {
    fn allocated_bytes(&self) -> u64 {
        let simulation = self.particle.as_ref().map_or(0, |particle| {
            u64::from(self.capacity)
                * PARTICLE_STRIDE_BYTES
                * (1 + particle.checkpoints.len() as u64)
        });
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

    fn source_matches(&self, source: &PointSceneSource) -> bool {
        match (&self.particle, source) {
            (Some(particle), PointSceneSource::Particle { parameters, .. }) => {
                particle.parameter_hash == stable_parameter_hash(parameters)
            }
            (None, PointSceneSource::Grid(_)) => true,
            _ => false,
        }
    }
}

/// Owns every mutable Particle buffer and every raw-GL object created beside
/// Ganesh. The caller guarantees that its glutin context is current.
pub(crate) struct SceneRuntime {
    gl: glow::Context,
    capability: Result<gl_backend::CapabilityProfile, String>,
    pipelines: HashMap<PointPipelineKey, PointPipeline>,
    invocations: HashMap<SceneInvocationKey, PointInvocation>,
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

    pub(crate) fn render_point(
        &mut self,
        scene: &PointSceneFrame,
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
            runtime.render_point_isolated(
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
    pub(crate) fn preflight_point(
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
            runtime.preflight_point_isolated(target_width, target_height, format)
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

    fn preflight_point_isolated(
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
        let pipeline = PointPipeline::create(&self.gl, use_tick, PointSourceKind::Particle, None)?;
        self.ensure_target(target_width, target_height, format)?;
        let buffer = match allocate_particle_buffer(&self.gl, 1) {
            Ok(buffer) => buffer,
            Err(error) => {
                pipeline.destroy(&self.gl);
                return Err(error);
            }
        };
        let result = (|| {
            let particle_pipeline = pipeline.particle.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Point preflight lost its Particle pipeline".to_string())
            })?;
            reset_particles(&self.gl, particle_pipeline, buffer, 1)?;
            let invocation = PointInvocation {
                particle: Some(ParticleState {
                    buffer,
                    parameter_hash: 0,
                    current_step: 0,
                    checkpoints: VecDeque::new(),
                }),
                point_fields: None,
                capacity: 1,
                executable_hash: [0; 32],
                last_used: use_tick,
            };
            let target = self.target.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Particle preflight target disappeared".to_string())
            })?;
            let point_source = PointSourceBinding::Particle { buffer };
            draw_points(
                &self.gl,
                &pipeline,
                PointDrawRequest {
                    invocation: &invocation,
                    point_source: &point_source,
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

    fn render_point_isolated(
        &mut self,
        scene: &PointSceneFrame,
        transform: &Affine2D,
        target_width: u32,
        target_height: u32,
        format: SceneTextureFormat,
        premultiplied_color: [f32; 4],
    ) -> Result<SceneTexture, LibraryError> {
        self.use_tick = self.use_tick.wrapping_add(1);
        let use_tick = self.use_tick;
        let source_kind = PointSourceKind::of(&scene.source);
        let pipeline = self.pipeline(source_kind, scene.point_program.as_ref(), use_tick)?;
        self.ensure_target(target_width, target_height, format)?;

        let capacity = scene.source.capacity();
        let mut invocation = match self.invocations.remove(&scene.invocation) {
            Some(invocation)
                if invocation.capacity == capacity
                    && invocation.executable_hash == scene.executable_hash
                    && invocation.source_matches(&scene.source)
                    && invocation.point_layout_matches(scene.point_program.as_ref()) =>
            {
                invocation
            }
            Some(invocation) => {
                self.destroy_invocation(invocation);
                self.reserve_invocation(&scene.source, scene.point_program.as_ref())?;
                self.create_invocation(scene, &pipeline, use_tick)?
            }
            None => {
                self.reserve_invocation(&scene.source, scene.point_program.as_ref())?;
                self.create_invocation(scene, &pipeline, use_tick)?
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
        let field_evaluation = if let (Some(program), Some(buffers), Some(point_pipeline)) = (
            scene.point_program.as_ref(),
            invocation.point_fields.as_ref(),
            pipeline.point_fields.as_ref(),
        ) {
            let point_source = point_source_binding(&invocation, &scene.source)?;
            point_pipeline.evaluate(
                &self.gl,
                program,
                buffers,
                &point_source,
                invocation_seed(scene),
            )
        } else {
            Ok(())
        };
        if let Err(error) = field_evaluation {
            self.destroy_invocation(invocation);
            return Err(error);
        }
        let point_source = point_source_binding(&invocation, &scene.source)?;
        let evaluation = (|| {
            let target = self.target.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Particle target disappeared before draw".to_string())
            })?;
            draw_points(
                &self.gl,
                &pipeline,
                PointDrawRequest {
                    invocation: &invocation,
                    point_source: &point_source,
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
        source_kind: PointSourceKind,
        point_program: Option<&crate::model::point::PointRenderProgram>,
        use_tick: u64,
    ) -> Result<PointPipeline, LibraryError> {
        let key = PointPipelineKey {
            source_kind,
            field_source_hash: point_fields::source_hash(source_kind, point_program)?,
        };
        if let Some(pipeline) = self.pipelines.get_mut(&key) {
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
        let pipeline = PointPipeline::create(&self.gl, use_tick, source_kind, point_program)?;
        self.pipelines.insert(key, pipeline.clone());
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
        scene: &PointSceneFrame,
        pipeline: &PointPipeline,
        last_used: u64,
    ) -> Result<PointInvocation, LibraryError> {
        let capacity = scene.source.capacity();
        let particle = match &scene.source {
            PointSceneSource::Particle { parameters, .. } => {
                let simulation = pipeline.particle.as_ref().ok_or_else(|| {
                    LibraryError::Render(
                        "GPU Point Particle source lost its simulation pipeline".to_string(),
                    )
                })?;
                let buffer = allocate_particle_buffer(&self.gl, capacity)?;
                if let Err(error) = reset_particles(&self.gl, simulation, buffer, capacity) {
                    delete_particle_buffer(&self.gl, buffer);
                    return Err(error);
                }
                Some(ParticleState {
                    buffer,
                    parameter_hash: stable_parameter_hash(parameters),
                    current_step: 0,
                    checkpoints: VecDeque::new(),
                })
            }
            PointSceneSource::Grid(_) => None,
        };
        let point_fields = match scene
            .point_program
            .as_ref()
            .map(|program| point_fields::PointFieldBuffers::create(&self.gl, program, capacity))
        {
            Some(Ok(buffers)) => Some(buffers),
            Some(Err(error)) => {
                if let Some(particle) = particle {
                    delete_particle_buffer(&self.gl, particle.buffer);
                }
                return Err(error);
            }
            None => None,
        };
        Ok(PointInvocation {
            particle,
            point_fields,
            capacity,
            executable_hash: scene.executable_hash,
            last_used,
        })
    }

    fn seek_invocation(
        &self,
        invocation: &mut PointInvocation,
        scene: &PointSceneFrame,
        pipeline: &PointPipeline,
    ) -> Result<(), LibraryError> {
        let PointSceneSource::Particle {
            target_step,
            parameters,
        } = &scene.source
        else {
            return Ok(());
        };
        let particle = invocation.particle.as_mut().ok_or_else(|| {
            LibraryError::Validation(
                "Particle source changed without rebuilding its invocation state".to_string(),
            )
        })?;
        let simulation = pipeline.particle.as_ref().ok_or_else(|| {
            LibraryError::Render("Particle source has no simulation pipeline".to_string())
        })?;
        if *target_step < particle.current_step {
            if let Some(checkpoint) = particle
                .checkpoints
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.step <= *target_step)
            {
                copy_particle_buffer(
                    &self.gl,
                    checkpoint.buffer,
                    particle.buffer,
                    invocation.capacity,
                )?;
                particle.current_step = checkpoint.step;
            } else {
                reset_particles(&self.gl, simulation, particle.buffer, invocation.capacity)?;
                particle.current_step = 0;
            }
        }
        if target_step.saturating_sub(particle.current_step) > PARTICLE_MAX_REPLAY_STEPS {
            // The executable slice has no persistent emitter state: a live
            // particle depends only on emissions within its maximum lifetime.
            // Reconstruct that bounded suffix with absolute step numbers so a
            // cold start or distant seek does not replay the entire Clip.
            reset_particles(&self.gl, simulation, particle.buffer, invocation.capacity)?;
            particle.current_step = bounded_replay_origin(parameters, *target_step);
        }
        validate_replay(particle.current_step, *target_step)?;
        while particle.current_step < *target_step {
            let until_checkpoint = PARTICLE_CHECKPOINT_INTERVAL_STEPS
                - particle.current_step % PARTICLE_CHECKPOINT_INTERVAL_STEPS;
            let count = (*target_step - particle.current_step).min(until_checkpoint);
            simulate_particles(
                &self.gl,
                simulation,
                ParticleSimulationRequest {
                    buffer: particle.buffer,
                    capacity: invocation.capacity,
                    seed: invocation_seed(scene),
                    start_step: particle.current_step,
                    step_count: count,
                    parameters,
                },
            )?;
            particle.current_step += count;
            if particle
                .current_step
                .is_multiple_of(PARTICLE_CHECKPOINT_INTERVAL_STEPS)
            {
                self.store_checkpoint(particle, invocation.capacity)?;
            }
        }
        Ok(())
    }

    fn store_checkpoint(
        &self,
        particle: &mut ParticleState,
        capacity: u32,
    ) -> Result<(), LibraryError> {
        while particle.checkpoints.len() >= PARTICLE_MAX_CHECKPOINTS {
            if let Some(checkpoint) = particle.checkpoints.pop_front() {
                delete_particle_buffer(&self.gl, checkpoint.buffer);
            }
        }
        let checkpoint_bytes = u64::from(capacity) * PARTICLE_STRIDE_BYTES;
        let resident_bytes = self
            .invocations
            .values()
            .map(PointInvocation::allocated_bytes)
            .sum::<u64>()
            .saturating_add(
                u64::from(capacity)
                    * PARTICLE_STRIDE_BYTES
                    * (1 + particle.checkpoints.len() as u64),
            );
        if resident_bytes.saturating_add(checkpoint_bytes) > self.limits.max_state_bytes {
            // Checkpoints are derived cache data. Skipping one preserves exact
            // forward simulation while respecting the hard memory budget.
            return Ok(());
        }
        let buffer = allocate_particle_buffer(&self.gl, capacity)?;
        if let Err(error) = copy_particle_buffer(&self.gl, particle.buffer, buffer, capacity) {
            delete_particle_buffer(&self.gl, buffer);
            return Err(error);
        }
        particle.checkpoints.push_back(ParticleCheckpoint {
            step: particle.current_step,
            buffer,
        });
        Ok(())
    }

    fn reserve_invocation(
        &mut self,
        source: &PointSceneSource,
        point_program: Option<&crate::model::point::PointRenderProgram>,
    ) -> Result<(), LibraryError> {
        let capacity = source.capacity();
        let required_bytes = point_fields::required_invocation_bytes(
            matches!(source, PointSceneSource::Particle { .. }),
            point_program,
            capacity,
        )?;
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
            .map(PointInvocation::allocated_bytes)
            .sum()
    }

    fn destroy_invocation(&self, invocation: PointInvocation) {
        if let Some(particle) = invocation.particle {
            delete_particle_buffer(&self.gl, particle.buffer);
            for checkpoint in particle.checkpoints {
                delete_particle_buffer(&self.gl, checkpoint.buffer);
            }
        }
        if let Some(point_fields) = invocation.point_fields {
            point_fields.destroy(&self.gl);
        }
    }
}

impl Drop for SceneRuntime {
    fn drop(&mut self) {
        for (_, invocation) in self.invocations.drain() {
            if let Some(particle) = invocation.particle {
                delete_particle_buffer(&self.gl, particle.buffer);
                for checkpoint in particle.checkpoints {
                    delete_particle_buffer(&self.gl, checkpoint.buffer);
                }
            }
            if let Some(point_fields) = invocation.point_fields {
                point_fields.destroy(&self.gl);
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

#[cfg(test)]
mod tests;
