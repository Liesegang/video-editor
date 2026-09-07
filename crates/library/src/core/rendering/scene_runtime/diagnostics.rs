//! Test-only counters observed on the production invocation owner.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PointInvocationStats {
    pub simulation_generation: u64,
    pub simulated_steps: u64,
    pub checkpoint_restores: u64,
    pub current_step: u64,
    pub checkpoint_steps: Vec<u64>,
    pub field_generation: u64,
    pub field_bytes: u64,
    pub connection_generation: u64,
    pub connection_bytes: u64,
    pub candidate_tests: u32,
    pub max_point_candidates: u32,
    pub compact_edge_count: u32,
    pub allocated_bytes: u64,
}

impl SceneRuntime {
    /// One-shot failure at the real field-evaluation boundary, after compute
    /// has written its derived buffers. This never bypasses GPU execution.
    pub(crate) fn fail_next_field_evaluation_for_test(&mut self) {
        self.fail_next_field_evaluation = true;
    }

    pub(crate) fn invocation_stats(
        &self,
        key: &SceneInvocationKey,
    ) -> Option<PointInvocationStats> {
        let invocation = self.invocations.get(key)?;
        let particle = invocation.particle.as_ref();
        Some(PointInvocationStats {
            simulation_generation: particle.map_or(0, |particle| particle.generation),
            simulated_steps: particle.map_or(0, |particle| particle.simulated_steps),
            checkpoint_restores: particle.map_or(0, |particle| particle.checkpoint_restores),
            current_step: particle.map_or(0, |particle| particle.current_step),
            checkpoint_steps: particle.map_or_else(Vec::new, |particle| {
                particle
                    .checkpoints
                    .iter()
                    .map(|checkpoint| checkpoint.step)
                    .collect()
            }),
            field_generation: invocation.field_generation,
            field_bytes: invocation.field_bytes(),
            connection_generation: invocation.connection_generation,
            connection_bytes: invocation
                .connections
                .as_ref()
                .map_or(0, proximity::PointConnectionBuffers::byte_len),
            candidate_tests: invocation
                .connections
                .as_ref()
                .map_or(0, |buffers| buffers.candidate_tests.get()),
            max_point_candidates: invocation
                .connections
                .as_ref()
                .map_or(0, |buffers| buffers.max_point_candidates.get()),
            compact_edge_count: invocation
                .connections
                .as_ref()
                .map_or(0, |buffers| buffers.compact_edge_count.get()),
            allocated_bytes: invocation.allocated_bytes(),
        })
    }

    pub(crate) fn set_limits_for_test(&mut self, limits: SceneRuntimeLimits) {
        self.limits = limits;
    }
}
