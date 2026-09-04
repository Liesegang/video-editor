//! Evaluated, non-persisted commands for the stateful GPU particle runtime.
//!
//! Authored topology and parameters stay in a `ModuleDefinition`. These values
//! are the compact command crossing the RenderPlan -> renderer boundary; no
//! particle array or GPU handle enters the Project model.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::authoring::{
    InstancePath, MediaTime, ModuleInstanceId, ModuleOutputId, RationalRate,
};
use crate::model::frame::color::Color;
use crate::model::property::Vec3;

pub const PARTICLE_FIXED_STEP_HZ: i64 = 120;
pub const PARTICLE_MAX_CAPACITY: u32 = 100_000;
pub const PARTICLE_MAX_REPLAY_STEPS: u64 = 14_400;
pub const PARTICLE_CHECKPOINT_INTERVAL_STEPS: u64 = 240;
pub const PARTICLE_MAX_CHECKPOINTS: usize = 8;

/// Mutable scene state address. Module-internal topology contributes only the
/// stable state slot; placement identity is always instance-scoped.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct SceneInvocationKey {
    pub instance_path: InstancePath,
    pub module_instance_id: ModuleInstanceId,
    pub state_slot_id: Uuid,
    pub output_id: ModuleOutputId,
}

/// Uniform-only controls sampled from published parameters at a fixed-step
/// boundary. Allocation-changing `capacity` is kept explicit for cache keys.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct ParticleSceneParameters {
    pub capacity: u32,
    pub emission_rate: ordered_float::OrderedFloat<f32>,
    pub lifetime_seconds: ordered_float::OrderedFloat<f32>,
    pub seed: u32,
    pub velocity_min: Vec3,
    pub velocity_max: Vec3,
    pub gravity: Vec3,
    pub drag: ordered_float::OrderedFloat<f32>,
    pub size_min: ordered_float::OrderedFloat<f32>,
    pub size_max: ordered_float::OrderedFloat<f32>,
    pub color: Color,
}

impl ParticleSceneParameters {
    pub fn validate(&self) -> Result<(), String> {
        if self.capacity == 0 || self.capacity > PARTICLE_MAX_CAPACITY {
            return Err(format!(
                "Particle capacity must be between 1 and {PARTICLE_MAX_CAPACITY}"
            ));
        }
        let finite = [
            self.emission_rate.into_inner(),
            self.lifetime_seconds.into_inner(),
            self.drag.into_inner(),
            self.size_min.into_inner(),
            self.size_max.into_inner(),
        ]
        .into_iter()
        .chain([
            self.velocity_min.x.into_inner() as f32,
            self.velocity_min.y.into_inner() as f32,
            self.velocity_min.z.into_inner() as f32,
            self.velocity_max.x.into_inner() as f32,
            self.velocity_max.y.into_inner() as f32,
            self.velocity_max.z.into_inner() as f32,
            self.gravity.x.into_inner() as f32,
            self.gravity.y.into_inner() as f32,
            self.gravity.z.into_inner() as f32,
        ]);
        if !finite.into_iter().all(f32::is_finite) {
            return Err("Particle parameters must be finite".to_string());
        }
        if !(0.0..=100_000.0).contains(&self.emission_rate.into_inner()) {
            return Err("Particle emission rate must be between 0 and 100000/s".to_string());
        }
        if !(1.0 / PARTICLE_FIXED_STEP_HZ as f32..=120.0)
            .contains(&self.lifetime_seconds.into_inner())
        {
            return Err("Particle lifetime must be between one fixed step and 120s".to_string());
        }
        if !(0.0..=100.0).contains(&self.drag.into_inner()) {
            return Err("Particle drag must be between 0 and 100".to_string());
        }
        if self.size_min.into_inner() <= 0.0
            || self.size_min > self.size_max
            || self.size_max.into_inner() > 512.0
        {
            return Err(
                "Particle size range must be positive, ordered, and at most 512px".to_string(),
            );
        }
        Ok(())
    }
}

/// One deterministic scene evaluation request. Preview and export both create
/// this exact command and pass it through the same renderer method.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct ParticleSceneFrame {
    pub invocation: SceneInvocationKey,
    pub executable_hash: [u8; 32],
    pub target_step: u64,
    pub logical_width: u32,
    pub logical_height: u32,
    pub parameters: ParticleSceneParameters,
}

impl ParticleSceneFrame {
    pub fn target_step_for_time(time: MediaTime) -> Result<u64, String> {
        let rate = RationalRate::new(PARTICLE_FIXED_STEP_HZ, 1)?;
        let step = time.checked_frame_index(rate)?;
        u64::try_from(step).map_err(|_| "Particle local time must be non-negative".to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.logical_width == 0 || self.logical_height == 0 {
            return Err("Particle render dimensions must be positive".to_string());
        }
        self.parameters.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_media_time_maps_to_fixed_step_without_float_rounding() {
        assert_eq!(
            ParticleSceneFrame::target_step_for_time(MediaTime::new(1, 30).unwrap()).unwrap(),
            4
        );
        assert_eq!(
            ParticleSceneFrame::target_step_for_time(MediaTime::new(1, 120).unwrap()).unwrap(),
            1
        );
    }
}
