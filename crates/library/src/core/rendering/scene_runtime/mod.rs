//! Stateful GPU execution boundary shared by preview and export renderers.

#[cfg(test)]
mod diagnostics;
mod forces;
mod gl_backend;
mod invocation;
mod point_fields;
#[cfg(test)]
pub(crate) use diagnostics::PointInvocationStats;
#[cfg(test)]
mod readback;
#[cfg(test)]
pub(crate) use readback::PointFieldReadback;
mod render;
mod shaders;
mod simulation;
mod source;

use crate::rendering::gl_resources::SavedGlState;
use invocation::PointInvocation;
use std::collections::HashMap;

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
    point_source_binding, validate_color, validate_replay, validate_target, validate_transform,
    vec3_f32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PointPipelineKey {
    source_kind: PointSourceKind,
    field_source_hash: Option<[u8; 32]>,
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
    #[cfg(test)]
    fail_next_field_evaluation: bool,
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
            #[cfg(test)]
            fail_next_field_evaluation: false,
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
            let target = self.target.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Particle preflight target disappeared".to_string())
            })?;
            let point_source = PointSourceBinding::Particle { buffer };
            draw_points(
                &self.gl,
                &pipeline,
                PointDrawRequest {
                    capacity: 1,
                    point_fields: None,
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
                if invocation.capacity == capacity && invocation.source_matches(scene) =>
            {
                invocation
            }
            Some(invocation) => {
                invocation.destroy(&self.gl);
                self.reserve_invocation(&scene.source, scene.point_program.as_ref())?;
                self.create_invocation(scene, &pipeline, use_tick)?
            }
            None => {
                self.reserve_invocation(&scene.source, scene.point_program.as_ref())?;
                self.create_invocation(scene, &pipeline, use_tick)?
            }
        };

        if let Err(error) = self.reconcile_fields(&mut invocation, scene.point_program.as_ref()) {
            self.invocations
                .insert(scene.invocation.clone(), invocation);
            return Err(error);
        }
        if let Err(error) = self.seek_invocation(&mut invocation, scene, &pipeline) {
            // Compute/reset/copy errors can leave the SSBO partially updated
            // without advancing `current_step`. Discard derived state so a
            // retry starts from a known cold buffer rather than compounding
            // the failed step.
            invocation.destroy(&self.gl);
            return Err(error);
        }
        let field_evaluation = (|| {
            if let (Some(program), Some(buffers), Some(point_pipeline)) = (
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
                )?;
                #[cfg(test)]
                if std::mem::take(&mut self.fail_next_field_evaluation) {
                    return Err(LibraryError::Render(
                        "injected Point field evaluation failure".to_string(),
                    ));
                }
            }
            Ok(())
        })();
        if let Err(error) = field_evaluation {
            // Render fields never mutate the simulation buffer. Retire only
            // their potentially partial results and preserve valid history.
            invocation.discard_fields(&self.gl);
            self.invocations
                .insert(scene.invocation.clone(), invocation);
            return Err(error);
        }
        let evaluation = (|| {
            let point_source = point_source_binding(&invocation, &scene.source)?;
            let target = self.target.as_ref().ok_or_else(|| {
                LibraryError::Render("GPU Particle target disappeared before draw".to_string())
            })?;
            draw_points(
                &self.gl,
                &pipeline,
                PointDrawRequest {
                    capacity: invocation.capacity,
                    point_fields: invocation.point_fields.as_ref(),
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
}

impl Drop for SceneRuntime {
    fn drop(&mut self) {
        for (_, invocation) in self.invocations.drain() {
            invocation.destroy(&self.gl);
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
