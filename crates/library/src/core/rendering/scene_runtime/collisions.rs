//! Bounded collision uniforms for the existing fixed-step Particle kernel.

use super::gl_backend::required_uniform;
use super::vectors::{normalized_direction, vec3_f32};
use crate::error::LibraryError;
use crate::model::frame::particle::{
    PARTICLE_MAX_COLLIDERS, PARTICLE_MAX_COLLISION_CONTACTS, ParticleCollider,
    ParticleCollisionMode,
};
use glow::HasContext;

#[derive(Clone)]
pub(super) struct CollisionUniformLocations {
    count: glow::UniformLocation,
    kinds: glow::UniformLocation,
    normals_radius: glow::UniformLocation,
    points_bounce: glow::UniformLocation,
    friction: glow::UniformLocation,
}

impl CollisionUniformLocations {
    pub fn new(gl: &glow::Context, program: glow::Program) -> Result<Self, LibraryError> {
        Ok(Self {
            count: required_uniform(gl, program, "uCollisionCount")?,
            kinds: required_uniform(gl, program, "uCollisionKinds[0]")?,
            normals_radius: required_uniform(gl, program, "uCollisionNormalsRadius[0]")?,
            points_bounce: required_uniform(gl, program, "uCollisionPointsBounce[0]")?,
            friction: required_uniform(gl, program, "uCollisionFriction[0]")?,
        })
    }
}

pub(super) struct CollisionUniformData {
    count: i32,
    kinds: [i32; PARTICLE_MAX_COLLIDERS],
    normals_radius: [f32; PARTICLE_MAX_COLLIDERS * 4],
    points_bounce: [f32; PARTICLE_MAX_COLLIDERS * 4],
    friction: [f32; PARTICLE_MAX_COLLIDERS],
}

impl CollisionUniformData {
    pub fn new(collisions: &[ParticleCollider]) -> Result<Self, LibraryError> {
        if collisions.len() > PARTICLE_MAX_COLLIDERS {
            return Err(LibraryError::Validation(
                "Particle collision stack exceeds GPU limit".into(),
            ));
        }
        let mut result = Self {
            count: collisions.len() as i32,
            kinds: [0; PARTICLE_MAX_COLLIDERS],
            normals_radius: [0.0; PARTICLE_MAX_COLLIDERS * 4],
            points_bounce: [0.0; PARTICLE_MAX_COLLIDERS * 4],
            friction: [0.0; PARTICLE_MAX_COLLIDERS],
        };
        for (index, collision) in collisions.iter().enumerate() {
            collision.validate().map_err(LibraryError::Validation)?;
            match collision {
                ParticleCollider::Plane {
                    plane_point,
                    plane_normal,
                    radius,
                    bounce,
                    friction,
                } => {
                    let [nx, ny, nz] =
                        normalized_direction(*plane_normal, "Particle plane normal")?;
                    let [px, py, pz] = vec3_f32(*plane_point, "plane point")?;
                    result.normals_radius[index * 4..index * 4 + 4].copy_from_slice(&[
                        nx,
                        ny,
                        nz,
                        radius.into_inner(),
                    ]);
                    result.points_bounce[index * 4..index * 4 + 4].copy_from_slice(&[
                        px,
                        py,
                        pz,
                        bounce.into_inner(),
                    ]);
                    result.friction[index] = friction.into_inner();
                }
                ParticleCollider::Sphere {
                    center,
                    radius,
                    particle_radius,
                    mode,
                    bounce,
                    friction,
                } => {
                    let [x, y, z] = vec3_f32(*center, "collision sphere center")?;
                    let boundary = match mode {
                        ParticleCollisionMode::Solid => {
                            result.kinds[index] = 1;
                            radius.into_inner() + particle_radius.into_inner()
                        }
                        ParticleCollisionMode::Container => {
                            result.kinds[index] = 2;
                            radius.into_inner() - particle_radius.into_inner()
                        }
                    };
                    result.normals_radius[index * 4 + 3] = boundary;
                    result.points_bounce[index * 4..index * 4 + 4].copy_from_slice(&[
                        x,
                        y,
                        z,
                        bounce.into_inner(),
                    ]);
                    result.friction[index] = friction.into_inner();
                }
            }
        }
        Ok(result)
    }

    pub fn upload(&self, gl: &glow::Context, locations: &CollisionUniformLocations) {
        // SAFETY: locations belong to the bound, owned compute program and
        // these fixed arrays have the same bound as the generated shader ABI.
        unsafe {
            gl.uniform_1_i32(Some(&locations.count), self.count);
            gl.uniform_1_i32_slice(Some(&locations.kinds), &self.kinds);
            gl.uniform_4_f32_slice(Some(&locations.normals_radius), &self.normals_radius);
            gl.uniform_4_f32_slice(Some(&locations.points_bounce), &self.points_bounce);
            gl.uniform_1_f32_slice(Some(&locations.friction), &self.friction);
        }
    }
}

pub(super) fn collision_source() -> String {
    format!(
        "#define MAX_PARTICLE_COLLIDERS {PARTICLE_MAX_COLLIDERS}\n#define MAX_COLLISION_CONTACTS {PARTICLE_MAX_COLLISION_CONTACTS}\n{COLLISION_GLSL}"
    )
}

const COLLISION_GLSL: &str = r#"
uniform int uCollisionCount;
// 0: Plane, 1: solid Sphere, 2: container Sphere.
uniform int uCollisionKinds[MAX_PARTICLE_COLLIDERS];
uniform vec4 uCollisionNormalsRadius[MAX_PARTICLE_COLLIDERS];
uniform vec4 uCollisionPointsBounce[MAX_PARTICLE_COLLIDERS];
uniform float uCollisionFriction[MAX_PARTICLE_COLLIDERS];

// World coordinates are producer-local pixels. This tolerance only accepts
// projection roundoff; it is not a visual-size-dependent collision radius.
const float COLLISION_DISTANCE_EPSILON = 0.0001;

float collision_surface(vec3 position, int index, out vec3 normal) {
    vec3 offset = position - uCollisionPointsBounce[index].xyz;
    if (uCollisionKinds[index] == 0) {
        normal = uCollisionNormalsRadius[index].xyz;
        return dot(offset, normal) - uCollisionNormalsRadius[index].w;
    }
    float distance = length(offset);
    // A solid Sphere's exact center has no unique radial direction. Use one
    // deterministic direction, independent of invocation/slot/velocity.
    normal = distance > 0.0 ? offset / distance : vec3(1.0, 0.0, 0.0);
    float orientation = uCollisionKinds[index] == 1 ? 1.0 : -1.0;
    normal *= orientation;
    return orientation * (distance - uCollisionNormalsRadius[index].w);
}

void collision_response(inout vec3 velocity, int index, vec3 normal) {
    float speed = dot(velocity, normal);
    if (speed >= 0.0) return;
    vec3 normalVelocity = normal * (speed / dot(normal, normal));
    vec3 tangentVelocity = velocity - normalVelocity;
    velocity = tangentVelocity * (1.0 - uCollisionFriction[index])
        - normalVelocity * uCollisionPointsBounce[index].w;
}

bool project_particle_contacts(inout vec3 position, inout vec3 velocity) {
    for (int pass = 0; pass < MAX_COLLISION_CONTACTS; ++pass) {
        bool projected = false;
        for (int index = 0; index < MAX_PARTICLE_COLLIDERS; ++index) {
            if (index >= uCollisionCount) break;
            vec3 normal;
            float distance = collision_surface(position, index, normal);
            if (distance < 0.0) {
                position -= normal * (distance / dot(normal, normal));
                collision_response(velocity, index, normal);
                projected = true;
            }
        }
        if (!projected) break;
    }
    for (int index = 0; index < MAX_PARTICLE_COLLIDERS; ++index) {
        if (index >= uCollisionCount) break;
        vec3 normal;
        float tolerance = uCollisionKinds[index] == 0 ? COLLISION_DISTANCE_EPSILON
            : min(COLLISION_DISTANCE_EPSILON, uCollisionNormalsRadius[index].w * 0.0001);
        if (collision_surface(position, index, normal) < -tolerance) return false;
    }
    return !any(isnan(position)) && !any(isinf(position))
        && !any(isnan(velocity)) && !any(isinf(velocity));
}

bool collision_time(vec3 position, vec3 velocity, int index, out float time) {
    vec3 normal;
    if (uCollisionKinds[index] == 0) {
        float distance = collision_surface(position, index, normal);
        float speed = dot(velocity, normal);
        if (speed >= 0.0) return false;
        time = max(0.0, distance) / -speed;
        return true;
    }
    // Components can be finite even when dot(velocity, velocity) overflows.
    // Normalize a scaled vector and divide the hit distance in two steps so
    // direct PointSceneFrame callers cannot silently tunnel at large speeds.
    float velocityScale = max(max(abs(velocity.x), abs(velocity.y)), abs(velocity.z));
    if (velocityScale == 0.0 || isnan(velocityScale) || isinf(velocityScale)) return false;
    vec3 direction = velocity / velocityScale;
    float directionLength = length(direction);
    direction /= directionLength;
    vec3 offset = position - uCollisionPointsBounce[index].xyz;
    bool solid = uCollisionKinds[index] == 1;
    if (solid && dot(offset, direction) >= 0.0) return false;
    // Closest approach avoids cancellation of large b*b and a*c terms for a
    // fast segment with both endpoints outside a small Sphere.
    float directionSquared = dot(direction, direction);
    float along = dot(offset, direction) / directionSquared;
    vec3 nearest = offset - direction * along;
    float radius = uCollisionNormalsRadius[index].w;
    float halfChordSquared = (radius * radius - dot(nearest, nearest)) / directionSquared;
    if (halfChordSquared < 0.0 || (solid && halfChordSquared == 0.0)) return false;
    float halfChord = sqrt(halfChordSquared);
    float travel = -along + (solid ? -halfChord : halfChord);
    time = (max(0.0, travel) / velocityScale) / directionLength;
    return true;
}

// Sweep the complete segment, choosing earliest time-of-impact across all
// shapes. Authored order breaks exact ties. A zero-time corner contact can
// consume another iteration without introducing an artificial time offset.
bool advance_particle_contacts(inout vec3 position, inout vec3 velocity, float remaining) {
    if (!project_particle_contacts(position, velocity)) return false;
    for (int contact = 0; contact < MAX_COLLISION_CONTACTS; ++contact) {
        if (remaining <= 0.0) break;
        int hit = -1;
        float hitTime = remaining;
        for (int index = 0; index < MAX_PARTICLE_COLLIDERS; ++index) {
            if (index >= uCollisionCount) break;
            float time;
            if (!collision_time(position, velocity, index, time)) continue;
            if (time <= remaining && (hit < 0 || time < hitTime)) {
                hit = index;
                hitTime = time;
            }
        }
        if (hit < 0) {
            position += velocity * remaining;
            remaining = 0.0;
            break;
        }
        position += velocity * hitTime;
        vec3 normal;
        float distance = collision_surface(position, hit, normal);
        position -= normal * (distance / dot(normal, normal));
        collision_response(velocity, hit, normal);
        remaining = max(0.0, remaining - hitTime);
    }
    // On contact-budget exhaustion keep the last safe contact position.
    return project_particle_contacts(position, velocity);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::Vec3;

    fn plane(normal: [f64; 3]) -> ParticleCollider {
        ParticleCollider::Plane {
            plane_point: Vec3 {
                x: 12.0.into(),
                y: 34.0.into(),
                z: 56.0.into(),
            },
            plane_normal: Vec3 {
                x: normal[0].into(),
                y: normal[1].into(),
                z: normal[2].into(),
            },
            radius: 7.0.into(),
            bounce: 0.5.into(),
            friction: 0.25.into(),
        }
    }

    #[test]
    fn collision_uniforms_preserve_order_and_normalize_before_float_transport() {
        let encoded =
            CollisionUniformData::new(&[plane([0.0, -1e-100, 0.0]), plane([3.0, 0.0, 4.0])])
                .unwrap();
        assert_eq!(encoded.count, 2);
        assert_eq!(&encoded.normals_radius[..4], &[0.0, -1.0, 0.0, 7.0]);
        assert_eq!(&encoded.normals_radius[4..8], &[0.6, 0.0, 0.8, 7.0]);
        assert_eq!(&encoded.points_bounce[..4], &[12.0, 34.0, 56.0, 0.5]);
        assert_eq!(&encoded.friction[..2], &[0.25; 2]);
        assert!(CollisionUniformData::new(&[plane([0.0; 3])]).is_err());
        assert!(
            CollisionUniformData::new(&vec![plane([0.0, 1.0, 0.0]); PARTICLE_MAX_COLLIDERS + 1])
                .is_err()
        );
    }

    #[test]
    fn mixed_collision_uniforms_encode_sphere_mode_and_physical_radius_once() {
        let sphere = |mode| ParticleCollider::Sphere {
            center: Vec3 {
                x: 12.0.into(),
                y: 34.0.into(),
                z: 56.0.into(),
            },
            radius: 20.0.into(),
            particle_radius: 3.0.into(),
            mode,
            bounce: 0.75.into(),
            friction: 0.4.into(),
        };
        let encoded = CollisionUniformData::new(&[
            plane([0.0, 1.0, 0.0]),
            sphere(ParticleCollisionMode::Solid),
            sphere(ParticleCollisionMode::Container),
        ])
        .unwrap();
        assert_eq!(encoded.count, 3);
        assert_eq!(&encoded.kinds[..3], &[0, 1, 2]);
        assert_eq!(&encoded.normals_radius[4..8], &[0.0, 0.0, 0.0, 23.0]);
        assert_eq!(&encoded.normals_radius[8..12], &[0.0, 0.0, 0.0, 17.0]);
        assert_eq!(&encoded.points_bounce[4..8], &[12.0, 34.0, 56.0, 0.75]);
        assert_eq!(&encoded.points_bounce[8..12], &[12.0, 34.0, 56.0, 0.75]);
        assert_eq!(&encoded.friction[1..3], &[0.4, 0.4]);
    }
}
