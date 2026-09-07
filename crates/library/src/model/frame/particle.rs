//! Evaluated, non-persisted commands for the stateful GPU particle runtime.
//!
//! Authored topology and parameters stay in a `ModuleDefinition`. These values
//! are the compact command crossing the RenderPlan -> renderer boundary; no
//! particle array or GPU handle enters the Project model.

use crate::model::authoring::{MediaTime, RationalRate};
use crate::model::property::Vec3;
use serde::{Deserialize, Serialize};

mod collision;
pub use collision::{
    PARTICLE_MAX_COLLIDERS, PARTICLE_MAX_COLLISION_CONTACTS, ParticleCollider,
    ParticleCollisionMode,
};

pub const PARTICLE_FIXED_STEP_HZ: i64 = 120;
pub const PARTICLE_MAX_CAPACITY: u32 = 100_000;
pub const PARTICLE_MAX_REPLAY_STEPS: u64 = 14_400;
pub const PARTICLE_MAX_COLD_REPLAY_PARTICLE_STEPS: u64 = 32 * 1024 * 1024;
pub const PARTICLE_CHECKPOINT_INTERVAL_STEPS: u64 = 240;
pub const PARTICLE_MAX_CHECKPOINTS: usize = 8;
pub const PARTICLE_MAX_FORCES: usize = 16;
pub const PARTICLE_MAX_TURBULENCE_OCTAVES: u32 = 4;

const PARTICLE_VECTOR_LIMIT: f64 = 1_000_000.0;
const PARTICLE_FORCE_LIMIT: f32 = 100_000.0;
const PARTICLE_FALLOFF_LIMIT: f32 = 16.0;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ParticleEmitterShape {
    Point,
    Box,
    Sphere,
}

/// One ordered force in the particle update stage.
///
/// Force order is authored by the Particle graph and retained by the runtime
/// command without regrouping force kinds.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParticleForce {
    Gravity {
        acceleration: Vec3,
    },
    Drag {
        coefficient: ordered_float::OrderedFloat<f32>,
    },
    Turbulence {
        strength: ordered_float::OrderedFloat<f32>,
        frequency: ordered_float::OrderedFloat<f32>,
        octaves: u32,
        evolution: ordered_float::OrderedFloat<f32>,
        seed: u32,
    },
    Vortex {
        axis: Vec3,
        center: Vec3,
        strength: ordered_float::OrderedFloat<f32>,
    },
    Point {
        target: Vec3,
        strength: ordered_float::OrderedFloat<f32>,
        radius: ordered_float::OrderedFloat<f32>,
        falloff: ordered_float::OrderedFloat<f32>,
    },
}

impl ParticleForce {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Gravity { acceleration } => {
                validate_force_vec3("Particle gravity acceleration", acceleration)
            }
            Self::Drag { coefficient } => {
                validate_f32("Particle drag coefficient", coefficient.into_inner())?;
                if !(0.0..=100.0).contains(&coefficient.into_inner()) {
                    return Err("Particle drag must be between 0 and 100".to_string());
                }
                Ok(())
            }
            Self::Turbulence {
                strength,
                frequency,
                octaves,
                evolution,
                ..
            } => {
                validate_f32("Particle turbulence strength", strength.into_inner())?;
                validate_f32("Particle turbulence frequency", frequency.into_inner())?;
                validate_f32("Particle turbulence evolution", evolution.into_inner())?;
                if !(0.0..=PARTICLE_FORCE_LIMIT).contains(&strength.into_inner()) {
                    return Err(
                        "Particle turbulence strength must be between 0 and 100000px/s²"
                            .to_string(),
                    );
                }
                if !(0.000_01..=1.0).contains(&frequency.into_inner()) {
                    return Err(
                        "Particle turbulence frequency must be between 0.00001 and 1 noise cells/px"
                            .to_string(),
                    );
                }
                if !(1..=PARTICLE_MAX_TURBULENCE_OCTAVES).contains(octaves) {
                    return Err("Particle turbulence octaves must be between 1 and 4".to_string());
                }
                if !(0.0..=100.0).contains(&evolution.into_inner()) {
                    return Err(
                        "Particle turbulence evolution must be between 0 and 100 cells/s"
                            .to_string(),
                    );
                }
                Ok(())
            }
            Self::Vortex {
                axis,
                center,
                strength,
            } => {
                validate_force_vec3("Particle vortex axis", axis)?;
                validate_force_vec3("Particle vortex center", center)?;
                validate_f32("Particle vortex strength", strength.into_inner())?;
                if vec3_is_zero(axis) {
                    return Err("Particle vortex axis must be non-zero".to_string());
                }
                if !(-PARTICLE_FORCE_LIMIT..=PARTICLE_FORCE_LIMIT).contains(&strength.into_inner())
                {
                    return Err(
                        "Particle vortex strength must be between -100000 and 100000px/s²"
                            .to_string(),
                    );
                }
                Ok(())
            }
            Self::Point {
                target,
                strength,
                radius,
                falloff,
            } => {
                validate_force_vec3("Particle point-force target", target)?;
                for (label, value) in [
                    ("Particle point-force strength", strength.into_inner()),
                    ("Particle point-force radius", radius.into_inner()),
                    ("Particle point-force falloff", falloff.into_inner()),
                ] {
                    validate_f32(label, value)?;
                }
                if !(-PARTICLE_FORCE_LIMIT..=PARTICLE_FORCE_LIMIT).contains(&strength.into_inner())
                {
                    return Err(
                        "Particle point-force strength must be between -100000 and 100000px/s²"
                            .to_string(),
                    );
                }
                if !(0.0..=PARTICLE_VECTOR_LIMIT as f32).contains(&radius.into_inner())
                    || radius.into_inner() == 0.0
                {
                    return Err(
                        "Particle point-force radius must be positive and at most 1000000px"
                            .to_string(),
                    );
                }
                if !(0.0..=PARTICLE_FALLOFF_LIMIT).contains(&falloff.into_inner()) {
                    return Err("Particle point-force falloff must be between 0 and 16".to_string());
                }
                Ok(())
            }
        }
    }
}

fn validate_f32(label: &str, value: f32) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{label} must be finite"));
    }
    Ok(())
}

fn validate_force_vec3(label: &str, value: &Vec3) -> Result<(), String> {
    let components = [
        value.x.into_inner(),
        value.y.into_inner(),
        value.z.into_inner(),
    ];
    if components.iter().any(|component| !component.is_finite()) {
        return Err(format!("{label} must be finite"));
    }
    if components
        .iter()
        .any(|component| !(-PARTICLE_VECTOR_LIMIT..=PARTICLE_VECTOR_LIMIT).contains(component))
    {
        return Err(format!(
            "{label} components must be between -1000000 and 1000000"
        ));
    }
    Ok(())
}

fn vec3_is_zero(value: &Vec3) -> bool {
    value.x.into_inner() == 0.0 && value.y.into_inner() == 0.0 && value.z.into_inner() == 0.0
}

pub(crate) fn validate_particle_size_range(size_min: f64, size_max: f64) -> Result<(), String> {
    if size_min <= 0.0 || size_min > size_max || size_max > 512.0 {
        return Err("Particle size range must be positive, ordered, and at most 512px".to_string());
    }
    Ok(())
}

pub(crate) fn particle_lifetime_steps(lifetime_seconds: f64) -> u64 {
    (lifetime_seconds * PARTICLE_FIXED_STEP_HZ as f64).ceil() as u64
}

pub(crate) fn validate_particle_cold_replay_budget(
    capacity: u32,
    lifetime_seconds: f64,
) -> Result<(), String> {
    let work = u64::from(capacity).saturating_mul(particle_lifetime_steps(lifetime_seconds));
    if work > PARTICLE_MAX_COLD_REPLAY_PARTICLE_STEPS {
        return Err(format!(
            "Particle capacity x lifetime requires {work} particle-steps for a cold seek, exceeding the {PARTICLE_MAX_COLD_REPLAY_PARTICLE_STEPS} work budget"
        ));
    }
    Ok(())
}

/// Conservatively estimates fixed-step work without charging every allocated
/// slot for force evaluation. The base simulation visits `capacity` slots;
/// modifiers run only for the bounded number of particles that emission and
/// lifetime can keep active. Turbulence charges its actual 3 gradients x 8
/// lattice corners per octave, simple forces charge one unit each, and each
/// collision charges the complete bounded contact kernel.
fn validate_particle_modifier_replay_budget(
    capacity: u32,
    emission_rate: f32,
    lifetime_seconds: f32,
    forces: &[ParticleForce],
    collisions: &[ParticleCollider],
) -> Result<(), String> {
    let lifetime_steps = particle_lifetime_steps(f64::from(lifetime_seconds));
    let fixed_step_seconds = 1.0 / PARTICLE_FIXED_STEP_HZ as f64;
    let emitted_during_lifetime =
        (f64::from(emission_rate) * (f64::from(lifetime_seconds) + fixed_step_seconds)).ceil();
    let active_bound = if emission_rate == 0.0 {
        0
    } else {
        u64::from(capacity).min(emitted_during_lifetime as u64 + 1)
    };
    let force_cost = forces.iter().fold(0_u64, |cost, force| {
        cost.saturating_add(match force {
            ParticleForce::Turbulence {
                strength, octaves, ..
            } if strength.into_inner() != 0.0 => 24 * u64::from(*octaves),
            ParticleForce::Turbulence { .. } => 0,
            ParticleForce::Gravity { .. } | ParticleForce::Drag { .. } => 1,
            ParticleForce::Vortex { strength, .. } | ParticleForce::Point { strength, .. }
                if strength.into_inner() != 0.0 =>
            {
                1
            }
            ParticleForce::Vortex { .. } | ParticleForce::Point { .. } => 0,
        })
    });
    let base_work = u64::from(capacity).saturating_mul(lifetime_steps);
    let collision_cost = collisions.iter().fold(0_u64, |cost, collision| {
        cost.saturating_add(collision.work_units())
    });
    let modifier_cost = force_cost.saturating_add(collision_cost);
    let modifier_work = active_bound
        .saturating_mul(lifetime_steps)
        .saturating_mul(modifier_cost);
    let estimated_work = base_work.saturating_add(modifier_work);
    if estimated_work > PARTICLE_MAX_COLD_REPLAY_PARTICLE_STEPS {
        return Err(format!(
            "Particle cold replay is estimated at {estimated_work} work units, exceeding the {PARTICLE_MAX_COLD_REPLAY_PARTICLE_STEPS} work budget"
        ));
    }
    Ok(())
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
    pub emitter_shape: ParticleEmitterShape,
    pub emitter_position: Vec3,
    pub emitter_radius: ordered_float::OrderedFloat<f32>,
    pub emitter_size: Vec3,
    pub emitter_surface_only: bool,
    pub velocity_min: Vec3,
    pub velocity_max: Vec3,
    pub forces: Vec<ParticleForce>,
    pub collisions: Vec<ParticleCollider>,
    pub size_min: ordered_float::OrderedFloat<f32>,
    pub size_max: ordered_float::OrderedFloat<f32>,
}

impl ParticleSceneParameters {
    pub fn target_step_for_time(time: MediaTime) -> Result<u64, String> {
        let rate = RationalRate::new(PARTICLE_FIXED_STEP_HZ, 1)?;
        let step = time.checked_frame_index(rate)?;
        u64::try_from(step).map_err(|_| "Particle local time must be non-negative".to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.capacity == 0 || self.capacity > PARTICLE_MAX_CAPACITY {
            return Err(format!(
                "Particle capacity must be between 1 and {PARTICLE_MAX_CAPACITY}"
            ));
        }
        let finite = [
            self.emission_rate.into_inner(),
            self.lifetime_seconds.into_inner(),
            self.emitter_radius.into_inner(),
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
            self.emitter_position.x.into_inner() as f32,
            self.emitter_position.y.into_inner() as f32,
            self.emitter_position.z.into_inner() as f32,
            self.emitter_size.x.into_inner() as f32,
            self.emitter_size.y.into_inner() as f32,
            self.emitter_size.z.into_inner() as f32,
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
        validate_particle_cold_replay_budget(
            self.capacity,
            f64::from(self.lifetime_seconds.into_inner()),
        )?;
        if self.forces.len() > PARTICLE_MAX_FORCES {
            return Err(format!(
                "Particle scenes support at most {PARTICLE_MAX_FORCES} ordered forces"
            ));
        }
        for force in &self.forces {
            force.validate()?;
        }
        if self.collisions.len() > PARTICLE_MAX_COLLIDERS {
            return Err(format!(
                "Particle scenes support at most {PARTICLE_MAX_COLLIDERS} ordered collisions"
            ));
        }
        for collision in &self.collisions {
            collision.validate()?;
        }
        validate_particle_modifier_replay_budget(
            self.capacity,
            self.emission_rate.into_inner(),
            self.lifetime_seconds.into_inner(),
            &self.forces,
            &self.collisions,
        )?;
        if !(0.0..=1_000_000.0).contains(&self.emitter_radius.into_inner()) {
            return Err("Particle emitter radius must be between 0 and 1000000px".to_string());
        }
        if [
            self.emitter_size.x.into_inner(),
            self.emitter_size.y.into_inner(),
            self.emitter_size.z.into_inner(),
        ]
        .into_iter()
        .any(|component| component < 0.0)
        {
            return Err("Particle emitter size components must be non-negative".to_string());
        }
        validate_particle_size_range(
            f64::from(self.size_min.into_inner()),
            f64::from(self.size_max.into_inner()),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;

    fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        }
    }

    fn valid_parameters() -> ParticleSceneParameters {
        ParticleSceneParameters {
            capacity: 128,
            emission_rate: OrderedFloat(10.0),
            lifetime_seconds: OrderedFloat(1.0),
            seed: 1,
            emitter_shape: ParticleEmitterShape::Point,
            emitter_position: vec3(0.0, 0.0, 0.0),
            emitter_radius: OrderedFloat(0.0),
            emitter_size: vec3(0.0, 0.0, 0.0),
            emitter_surface_only: false,
            velocity_min: vec3(0.0, 0.0, 0.0),
            velocity_max: vec3(0.0, 0.0, 0.0),
            forces: Vec::new(),
            collisions: Vec::new(),
            size_min: OrderedFloat(1.0),
            size_max: OrderedFloat(1.0),
        }
    }

    fn plane_collision() -> ParticleCollider {
        ParticleCollider::Plane {
            plane_point: vec3(0.0, 0.0, 0.0),
            plane_normal: vec3(0.0, -1.0, 0.0),
            radius: OrderedFloat(0.0),
            bounce: OrderedFloat(0.5),
            friction: OrderedFloat(0.1),
        }
    }

    fn sphere_collision() -> ParticleCollider {
        ParticleCollider::Sphere {
            center: vec3(0.0, 120.0, 0.0),
            radius: OrderedFloat(100.0),
            particle_radius: OrderedFloat(0.0),
            mode: ParticleCollisionMode::Solid,
            bounce: OrderedFloat(0.5),
            friction: OrderedFloat(0.1),
        }
    }

    #[test]
    fn exact_media_time_maps_to_fixed_step_without_float_rounding() {
        assert_eq!(
            ParticleSceneParameters::target_step_for_time(MediaTime::new(1, 30).unwrap()).unwrap(),
            4
        );
        assert_eq!(
            ParticleSceneParameters::target_step_for_time(MediaTime::new(1, 120).unwrap()).unwrap(),
            1
        );
    }

    #[test]
    fn force_values_round_trip_and_validate_their_runtime_bounds() {
        let forces = vec![
            ParticleForce::Gravity {
                acceleration: vec3(0.0, 180.0, 0.0),
            },
            ParticleForce::Drag {
                coefficient: OrderedFloat(0.15),
            },
            ParticleForce::Turbulence {
                strength: OrderedFloat(500.0),
                frequency: OrderedFloat(0.01),
                octaves: 4,
                evolution: OrderedFloat(2.0),
                seed: u32::MAX,
            },
            ParticleForce::Vortex {
                axis: vec3(0.0, 0.0, 1.0),
                center: vec3(320.0, 180.0, 0.0),
                strength: OrderedFloat(1_000.0),
            },
            ParticleForce::Point {
                target: vec3(320.0, 180.0, 0.0),
                strength: OrderedFloat(-1_000.0),
                radius: OrderedFloat(240.0),
                falloff: OrderedFloat(2.0),
            },
        ];
        for force in forces {
            force.validate().expect("valid force");
            let json = serde_json::to_string(&force).expect("serialize force");
            let decoded: ParticleForce = serde_json::from_str(&json).expect("deserialize force");
            assert_eq!(decoded, force);
        }
    }

    #[test]
    fn force_validation_rejects_unbounded_and_degenerate_values() {
        assert!(
            ParticleForce::Vortex {
                axis: vec3(0.0, 0.0, 0.0),
                center: vec3(0.0, 0.0, 0.0),
                strength: OrderedFloat(1.0),
            }
            .validate()
            .unwrap_err()
            .contains("non-zero")
        );
        assert!(
            ParticleForce::Point {
                target: vec3(0.0, 0.0, 0.0),
                strength: OrderedFloat(-1.0),
                radius: OrderedFloat(0.0),
                falloff: OrderedFloat(2.0),
            }
            .validate()
            .unwrap_err()
            .contains("radius")
        );
        assert!(
            ParticleForce::Turbulence {
                strength: OrderedFloat(1.0),
                frequency: OrderedFloat(0.0),
                octaves: PARTICLE_MAX_TURBULENCE_OCTAVES + 1,
                evolution: OrderedFloat(0.0),
                seed: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            ParticleForce::Gravity {
                acceleration: vec3(f64::NAN, 0.0, 0.0),
            }
            .validate()
            .unwrap_err()
            .contains("finite")
        );
    }

    #[test]
    fn scene_validation_caps_the_ordered_force_program() {
        let mut parameters = valid_parameters();
        parameters.forces = (0..PARTICLE_MAX_FORCES)
            .map(|_| ParticleForce::Drag {
                coefficient: OrderedFloat(0.0),
            })
            .collect();
        parameters.validate().expect("maximum force count");
        parameters.forces.push(ParticleForce::Drag {
            coefficient: OrderedFloat(0.0),
        });
        assert!(parameters.validate().unwrap_err().contains("at most 16"));
    }

    #[test]
    fn scene_validation_caps_the_ordered_collision_program() {
        let plane = plane_collision();
        let mut parameters = valid_parameters();
        parameters.collisions = vec![plane.clone(); PARTICLE_MAX_COLLIDERS];
        parameters.validate().expect("maximum collision count");
        parameters.collisions.push(plane);
        assert!(parameters.validate().unwrap_err().contains("at most 8"));
    }

    #[test]
    fn scene_collision_budget_charges_the_bounded_contact_kernel() {
        let mut parameters = valid_parameters();
        parameters.capacity = 8_192;
        parameters.emission_rate = OrderedFloat(120.0);
        parameters.lifetime_seconds = OrderedFloat(4.0);
        parameters.collisions = vec![plane_collision()];
        parameters
            .validate()
            .expect("one collision fits the default Particle workload");

        parameters.collisions = vec![plane_collision(); PARTICLE_MAX_COLLIDERS];
        assert!(parameters.validate().unwrap_err().contains("work budget"));

        parameters.emission_rate = OrderedFloat(0.0);
        parameters
            .validate()
            .expect("collisions add no active-particle work without emission");
    }

    #[test]
    fn scene_collision_budget_charges_sphere_contact_work_per_kind() {
        let mut parameters = valid_parameters();
        parameters.capacity = 8_192;
        parameters.emission_rate = OrderedFloat(120.0);
        parameters.lifetime_seconds = OrderedFloat(4.0);
        parameters.collisions = vec![sphere_collision()];
        parameters
            .validate()
            .expect("one Sphere fits the default Particle workload");

        parameters.collisions = vec![sphere_collision(); PARTICLE_MAX_COLLIDERS];
        assert!(parameters.validate().unwrap_err().contains("work budget"));

        parameters.emission_rate = OrderedFloat(0.0);
        parameters
            .validate()
            .expect("inactive Sphere stages add no active-particle work without emission");
    }

    #[test]
    fn scene_force_budget_uses_active_particles_and_octave_cost() {
        let mut parameters = valid_parameters();
        parameters.capacity = 8_192;
        parameters.emission_rate = OrderedFloat(120.0);
        parameters.lifetime_seconds = OrderedFloat(4.0);
        parameters.forces = vec![
            ParticleForce::Gravity {
                acceleration: vec3(0.0, 180.0, 0.0),
            },
            ParticleForce::Turbulence {
                strength: OrderedFloat(120.0),
                frequency: OrderedFloat(0.01),
                octaves: 4,
                evolution: OrderedFloat(0.0),
                seed: 1,
            },
            ParticleForce::Drag {
                coefficient: OrderedFloat(0.15),
            },
        ];
        parameters
            .validate()
            .expect("one four-octave Turbulence stays inside the bounded default workload");

        parameters.forces = (0..PARTICLE_MAX_FORCES)
            .map(|seed| ParticleForce::Turbulence {
                strength: OrderedFloat(120.0),
                frequency: OrderedFloat(0.01),
                octaves: 4,
                evolution: OrderedFloat(0.0),
                seed: seed as u32,
            })
            .collect();
        assert!(parameters.validate().unwrap_err().contains("work budget"));
    }
}
