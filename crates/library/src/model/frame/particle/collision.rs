//! Evaluated collision commands for the fixed-step Particle runtime.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::property::Vec3;

use super::{PARTICLE_VECTOR_LIMIT, validate_f32, validate_force_vec3, vec3_is_zero};

pub const PARTICLE_MAX_COLLIDERS: usize = 8;
pub const PARTICLE_MAX_COLLISION_CONTACTS: usize = 8;

/// Conservative per-plane work charged by the cold-replay guard: each of two
/// projection calls performs up to `I` passes plus one final scan; the sweep
/// performs `I` time-of-impact scans and up to `I` hit projections.
pub(crate) const PARTICLE_COLLISION_WORK_UNITS_PER_STAGE: u64 =
    4 * PARTICLE_MAX_COLLISION_CONTACTS as u64 + 2;

/// One collision stage in authored upstream-to-downstream Node order.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParticleCollider {
    Plane {
        plane_point: Vec3,
        plane_normal: Vec3,
        radius: OrderedFloat<f32>,
        bounce: OrderedFloat<f32>,
        friction: OrderedFloat<f32>,
    },
}

impl ParticleCollider {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Plane {
                plane_point,
                plane_normal,
                radius,
                bounce,
                friction,
            } => {
                validate_force_vec3("Particle collision plane point", plane_point)?;
                validate_force_vec3("Particle collision plane normal", plane_normal)?;
                if vec3_is_zero(plane_normal) {
                    return Err("Particle collision plane normal must be non-zero".to_string());
                }
                for (label, value) in [
                    ("Particle collision radius", radius.into_inner()),
                    ("Particle collision bounce", bounce.into_inner()),
                    ("Particle collision friction", friction.into_inner()),
                ] {
                    validate_f32(label, value)?;
                }
                if !(0.0..=PARTICLE_VECTOR_LIMIT as f32).contains(&radius.into_inner()) {
                    return Err(
                        "Particle collision radius must be between 0 and 1000000px".to_string()
                    );
                }
                if !(0.0..=1.0).contains(&bounce.into_inner()) {
                    return Err("Particle collision bounce must be between 0 and 1".to_string());
                }
                if !(0.0..=1.0).contains(&friction.into_inner()) {
                    return Err("Particle collision friction must be between 0 and 1".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        }
    }

    fn plane() -> ParticleCollider {
        ParticleCollider::Plane {
            plane_point: vec3(0.0, 120.0, 0.0),
            plane_normal: vec3(0.0, -1.0, 0.0),
            radius: OrderedFloat(0.0),
            bounce: OrderedFloat(0.5),
            friction: OrderedFloat(0.1),
        }
    }

    #[test]
    fn plane_collision_round_trips_and_accepts_boundary_values() {
        let value = plane();
        value.validate().expect("valid Plane collision");
        let encoded = serde_json::to_string(&value).expect("serialize Plane collision");
        assert_eq!(
            serde_json::from_str::<ParticleCollider>(&encoded).expect("deserialize collision"),
            value
        );

        let mut boundary = plane();
        let ParticleCollider::Plane {
            radius,
            bounce,
            friction,
            ..
        } = &mut boundary;
        *radius = OrderedFloat(1_000_000.0);
        *bounce = OrderedFloat(1.0);
        *friction = OrderedFloat(0.0);
        boundary.validate().expect("valid boundary response");
    }

    #[test]
    fn plane_collision_rejects_invalid_geometry_and_response() {
        let invalid = [
            ParticleCollider::Plane {
                plane_point: vec3(0.0, 120.0, 0.0),
                plane_normal: vec3(0.0, 0.0, 0.0),
                radius: OrderedFloat(0.0),
                bounce: OrderedFloat(0.5),
                friction: OrderedFloat(0.1),
            },
            ParticleCollider::Plane {
                plane_point: vec3(0.0, 120.0, 0.0),
                plane_normal: vec3(0.0, -1.0, 0.0),
                radius: OrderedFloat(-1.0),
                bounce: OrderedFloat(0.5),
                friction: OrderedFloat(0.1),
            },
            ParticleCollider::Plane {
                plane_point: vec3(0.0, 120.0, 0.0),
                plane_normal: vec3(0.0, -1.0, 0.0),
                radius: OrderedFloat(0.0),
                bounce: OrderedFloat(1.01),
                friction: OrderedFloat(0.1),
            },
            ParticleCollider::Plane {
                plane_point: vec3(0.0, 120.0, 0.0),
                plane_normal: vec3(0.0, -1.0, 0.0),
                radius: OrderedFloat(0.0),
                bounce: OrderedFloat(0.5),
                friction: OrderedFloat(f32::NAN),
            },
        ];
        assert!(invalid.into_iter().all(|value| value.validate().is_err()));
    }

    #[test]
    fn unknown_collision_fields_fail_closed() {
        let mut encoded = serde_json::to_value(plane()).expect("collision JSON");
        encoded
            .as_object_mut()
            .expect("collision object")
            .insert("unknown".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<ParticleCollider>(encoded).is_err());
    }
}
