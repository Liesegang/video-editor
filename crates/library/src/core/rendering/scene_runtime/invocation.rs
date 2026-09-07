//! Invocation-owned simulation, render buffers, and bounded seek history.

use std::collections::VecDeque;

use super::*;
use crate::model::point::PointRenderProgram;

pub(super) struct ParticleCheckpoint {
    pub step: u64,
    pub buffer: glow::Buffer,
}

/// Only dependencies of the current trusted, constant-input simulation.
/// Instance-dependent random inputs are already scoped by SceneInvocationKey.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParticleSimulationIdentity {
    pub source_node_id: uuid::Uuid,
    pub parameters: ParticleSceneParameters,
}

pub(super) struct ParticleState {
    pub buffer: glow::Buffer,
    pub identity: ParticleSimulationIdentity,
    pub current_step: u64,
    pub checkpoints: VecDeque<ParticleCheckpoint>,
    #[cfg(test)]
    pub generation: u64,
    #[cfg(test)]
    pub simulated_steps: u64,
    #[cfg(test)]
    pub checkpoint_restores: u64,
}

impl ParticleState {
    fn allocated_bytes(&self) -> u64 {
        u64::from(self.identity.parameters.capacity)
            * PARTICLE_STRIDE_BYTES
            * (1 + self.checkpoints.len() as u64)
    }

    fn destroy(self, gl: &glow::Context) {
        delete_particle_buffer(gl, self.buffer);
        for checkpoint in self.checkpoints {
            delete_particle_buffer(gl, checkpoint.buffer);
        }
    }
}

pub(super) struct PointInvocation {
    pub particle: Option<ParticleState>,
    pub point_fields: Option<point_fields::PointFieldBuffers>,
    pub capacity: u32,
    pub last_used: u64,
    #[cfg(test)]
    pub field_generation: u64,
}

impl PointInvocation {
    pub fn field_bytes(&self) -> u64 {
        self.point_fields
            .as_ref()
            .map_or(0, point_fields::PointFieldBuffers::byte_len)
    }

    pub fn allocated_bytes(&self) -> u64 {
        self.particle
            .as_ref()
            .map_or(0, ParticleState::allocated_bytes)
            .saturating_add(self.field_bytes())
    }

    fn point_layout_matches(&self, program: Option<&PointRenderProgram>) -> bool {
        match (&self.point_fields, program) {
            (None, None) => true,
            (Some(buffers), Some(program)) => buffers.matches(program),
            _ => false,
        }
    }

    pub fn source_matches(&self, scene: &PointSceneFrame) -> bool {
        match (&self.particle, &scene.source) {
            (Some(particle), PointSceneSource::Particle { parameters, .. }) => {
                particle.identity.source_node_id == scene.source_node_id
                    && particle.identity.parameters == *parameters
            }
            (None, PointSceneSource::Grid(_)) => true,
            _ => false,
        }
    }

    pub fn discard_fields(&mut self, gl: &glow::Context) {
        if let Some(fields) = self.point_fields.take() {
            fields.destroy(gl);
        }
        #[cfg(test)]
        {
            self.field_generation = 0;
        }
    }

    pub fn destroy(mut self, gl: &glow::Context) {
        self.discard_fields(gl);
        if let Some(particle) = self.particle {
            particle.destroy(gl);
        }
    }
}

impl SceneRuntime {
    pub(super) fn create_invocation(
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
                    identity: ParticleSimulationIdentity {
                        source_node_id: scene.source_node_id,
                        parameters: parameters.clone(),
                    },
                    current_step: 0,
                    checkpoints: VecDeque::new(),
                    #[cfg(test)]
                    generation: last_used,
                    #[cfg(test)]
                    simulated_steps: 0,
                    #[cfg(test)]
                    checkpoint_restores: 0,
                })
            }
            PointSceneSource::Grid(_) => None,
        };
        let point_fields = match scene
            .point_program
            .as_ref()
            .map(|program| point_fields::PointFieldBuffers::create(&self.gl, program, capacity))
            .transpose()
        {
            Ok(buffers) => buffers,
            Err(error) => {
                if let Some(particle) = particle {
                    particle.destroy(&self.gl);
                }
                return Err(error);
            }
        };
        Ok(PointInvocation {
            particle,
            point_fields,
            capacity,
            last_used,
            #[cfg(test)]
            field_generation: if scene.point_program.is_some() {
                last_used
            } else {
                0
            },
        })
    }

    pub(super) fn reconcile_fields(
        &mut self,
        invocation: &mut PointInvocation,
        program: Option<&PointRenderProgram>,
    ) -> Result<(), LibraryError> {
        if invocation.point_layout_matches(program) {
            return Ok(());
        }
        let replacement = if let Some(program) = program {
            let field_bytes =
                point_fields::required_invocation_bytes(false, Some(program), invocation.capacity)?;
            // The active invocation has been removed from the LRU map. Count
            // all its buffers plus the replacement until allocation commits.
            self.reserve_state(invocation.allocated_bytes().saturating_add(field_bytes))?;
            Some(point_fields::PointFieldBuffers::create(
                &self.gl,
                program,
                invocation.capacity,
            )?)
        } else {
            None
        };
        invocation.discard_fields(&self.gl);
        invocation.point_fields = replacement;
        #[cfg(test)]
        {
            invocation.field_generation = if program.is_some() { self.use_tick } else { 0 };
        }
        Ok(())
    }

    pub(super) fn seek_invocation(
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
        let field_bytes = invocation.field_bytes();
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
                #[cfg(test)]
                {
                    particle.checkpoint_restores += 1;
                }
            } else {
                reset_particles(&self.gl, simulation, particle.buffer, invocation.capacity)?;
                particle.current_step = 0;
            }
        }
        if target_step.saturating_sub(particle.current_step) > PARTICLE_MAX_REPLAY_STEPS {
            // Static independent particles depend only on emissions within the
            // maximum lifetime; no Birth/Update accumulation is accepted yet.
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
            #[cfg(test)]
            {
                particle.simulated_steps += count;
            }
            if particle
                .current_step
                .is_multiple_of(PARTICLE_CHECKPOINT_INTERVAL_STEPS)
            {
                self.store_checkpoint(particle, field_bytes)?;
            }
        }
        Ok(())
    }

    fn store_checkpoint(
        &self,
        particle: &mut ParticleState,
        field_bytes: u64,
    ) -> Result<(), LibraryError> {
        // Replaying a known step does not need a second identical checkpoint.
        if particle
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.step == particle.current_step)
        {
            return Ok(());
        }
        while particle.checkpoints.len() >= PARTICLE_MAX_CHECKPOINTS {
            if let Some(checkpoint) = particle.checkpoints.pop_front() {
                delete_particle_buffer(&self.gl, checkpoint.buffer);
            }
        }
        let capacity = particle.identity.parameters.capacity;
        let checkpoint_bytes = u64::from(capacity) * PARTICLE_STRIDE_BYTES;
        let resident_bytes = self
            .resident_state_bytes()
            .saturating_add(particle.allocated_bytes())
            .saturating_add(field_bytes);
        if resident_bytes.saturating_add(checkpoint_bytes) > self.limits.max_state_bytes {
            return Ok(()); // A checkpoint is optional derived cache data.
        }
        let buffer = allocate_particle_buffer(&self.gl, capacity)?;
        if let Err(error) = copy_particle_buffer(&self.gl, particle.buffer, buffer, capacity) {
            delete_particle_buffer(&self.gl, buffer);
            return Err(error);
        }
        let index = particle
            .checkpoints
            .partition_point(|checkpoint| checkpoint.step < particle.current_step);
        particle.checkpoints.insert(
            index,
            ParticleCheckpoint {
                step: particle.current_step,
                buffer,
            },
        );
        Ok(())
    }

    pub(super) fn reserve_invocation(
        &mut self,
        source: &PointSceneSource,
        program: Option<&PointRenderProgram>,
    ) -> Result<(), LibraryError> {
        self.reserve_state(point_fields::required_invocation_bytes(
            matches!(source, PointSceneSource::Particle { .. }),
            program,
            source.capacity(),
        )?)
    }

    /// Reserve all bytes outside the map, including an active invocation when
    /// swapping fields. It occupies one live slot, just like a new invocation.
    fn reserve_state(&mut self, required_bytes: u64) -> Result<(), LibraryError> {
        if required_bytes > self.limits.max_state_bytes {
            return Err(LibraryError::Render(format!(
                "GPU Point invocation requires {required_bytes} peak bytes, exceeding the configured {}-byte state budget",
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
                    "GPU Point state budget cannot admit an invocation".to_string(),
                ));
            };
            if let Some(invocation) = self.invocations.remove(&key) {
                invocation.destroy(&self.gl);
            }
        }
        Ok(())
    }

    pub(crate) fn resident_state_bytes(&self) -> u64 {
        self.invocations
            .values()
            .map(PointInvocation::allocated_bytes)
            .sum()
    }
}
