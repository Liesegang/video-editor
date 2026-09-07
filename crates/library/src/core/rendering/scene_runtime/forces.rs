//! Ordered, bounded force instructions for the shared Particle GPU kernel.
//!
//! This is a derived uniform ABI, not another authored Particle settings model.

use glow::HasContext;

use crate::error::LibraryError;
use crate::model::frame::particle::{PARTICLE_MAX_FORCES, ParticleForce};

use super::gl_backend::required_uniform;
use super::vec3_f32;
use super::vectors::normalized_direction;

#[derive(Clone)]
pub(super) struct ForceUniformLocations {
    count: glow::UniformLocation,
    kinds: glow::UniformLocation,
    seeds: glow::UniformLocation,
    first: glow::UniformLocation,
    second: glow::UniformLocation,
}

impl ForceUniformLocations {
    pub fn new(gl: &glow::Context, program: glow::Program) -> Result<Self, LibraryError> {
        Ok(Self {
            count: required_uniform(gl, program, "uForceCount")?,
            kinds: required_uniform(gl, program, "uForceKinds[0]")?,
            seeds: required_uniform(gl, program, "uForceSeeds[0]")?,
            first: required_uniform(gl, program, "uForceFirst[0]")?,
            second: required_uniform(gl, program, "uForceSecond[0]")?,
        })
    }
}

pub(super) struct ForceUniformData {
    count: i32,
    kinds: [i32; PARTICLE_MAX_FORCES],
    seeds: [u32; PARTICLE_MAX_FORCES],
    first: [f32; PARTICLE_MAX_FORCES * 4],
    second: [f32; PARTICLE_MAX_FORCES * 4],
}

impl ForceUniformData {
    pub fn new(forces: &[ParticleForce]) -> Result<Self, LibraryError> {
        if forces.len() > PARTICLE_MAX_FORCES {
            return Err(LibraryError::Validation(
                "Particle force stack exceeds GPU limit".into(),
            ));
        }
        let mut result = Self {
            count: forces.len() as i32,
            kinds: [0; PARTICLE_MAX_FORCES],
            seeds: [0; PARTICLE_MAX_FORCES],
            first: [0.0; PARTICLE_MAX_FORCES * 4],
            second: [0.0; PARTICLE_MAX_FORCES * 4],
        };
        for (index, force) in forces.iter().enumerate() {
            let (kind, first, second, seed) = match force {
                ParticleForce::Gravity { acceleration } => {
                    let [x, y, z] = vec3_f32(*acceleration, "gravity")?;
                    (0, [x, y, z, 0.0], [0.0; 4], 0)
                }
                ParticleForce::Drag { coefficient } => {
                    (1, [coefficient.into_inner(), 0.0, 0.0, 0.0], [0.0; 4], 0)
                }
                ParticleForce::Turbulence {
                    strength,
                    frequency,
                    octaves,
                    evolution,
                    seed,
                } => (
                    2,
                    [
                        strength.into_inner(),
                        frequency.into_inner(),
                        evolution.into_inner(),
                        *octaves as f32,
                    ],
                    [0.0; 4],
                    *seed,
                ),
                ParticleForce::Vortex {
                    axis,
                    center,
                    strength,
                } => {
                    let [x, y, z] = normalized_direction(*axis, "Particle vortex axis")?;
                    let [cx, cy, cz] = vec3_f32(*center, "vortex center")?;
                    (3, [x, y, z, strength.into_inner()], [cx, cy, cz, 0.0], 0)
                }
                ParticleForce::Point {
                    target,
                    strength,
                    radius,
                    falloff,
                } => {
                    let [x, y, z] = vec3_f32(*target, "point target")?;
                    (
                        4,
                        [x, y, z, strength.into_inner()],
                        [radius.into_inner(), falloff.into_inner(), 0.0, 0.0],
                        0,
                    )
                }
            };
            result.kinds[index] = kind;
            result.seeds[index] = seed;
            result.first[index * 4..index * 4 + 4].copy_from_slice(&first);
            result.second[index * 4..index * 4 + 4].copy_from_slice(&second);
        }
        Ok(result)
    }

    /// The caller has bound the validated, owning compute program/context.
    pub fn upload(&self, gl: &glow::Context, locations: &ForceUniformLocations) {
        // SAFETY: these locations belong to the active trusted Particle program.
        // Array lengths share the same bound as the generated shader declaration.
        unsafe {
            gl.uniform_1_i32(Some(&locations.count), self.count);
            gl.uniform_1_i32_slice(Some(&locations.kinds), &self.kinds);
            gl.uniform_1_u32_slice(Some(&locations.seeds), &self.seeds);
            gl.uniform_4_f32_slice(Some(&locations.first), &self.first);
            gl.uniform_4_f32_slice(Some(&locations.second), &self.second);
        }
    }
}

// Analytic derivatives of smooth value noise provide a curl field without
// finite-difference evaluations. Following Bridson et al., curl of the vector
// potential gives swirling motion without introducing a divergent random push.
// No simulation path depends on wall-clock time or draw order.
pub(super) const FORCE_GLSL: &str = r#"
uniform int uForceCount;
uniform int uForceKinds[MAX_PARTICLE_FORCES];
uniform uint uForceSeeds[MAX_PARTICLE_FORCES];
uniform vec4 uForceFirst[MAX_PARTICLE_FORCES];
uniform vec4 uForceSecond[MAX_PARTICLE_FORCES];

float lattice_noise(ivec3 cell, uint seed) {
    uvec3 p = uvec3(cell) & uvec3(255u);
    uint bits = hash_u32(seed ^ hash_u32(p.x) ^ hash_u32(p.y + 0x9e3779b9u)
        ^ hash_u32(p.z + 0x85ebca6bu));
    return float(bits & 0x00ffffffu) / 8388608.0 - 1.0;
}

vec3 noise_gradient(vec3 position, uint seed) {
    vec3 q = mod(position, vec3(256.0));
    ivec3 cell = ivec3(floor(q));
    vec3 f = fract(q);
    vec3 blend = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    vec3 derivative = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    vec3 gradient = vec3(0.0);
    for (int z = 0; z < 2; ++z) {
        for (int y = 0; y < 2; ++y) {
            for (int x = 0; x < 2; ++x) {
                vec3 corner = vec3(x, y, z);
                vec3 weights = mix(vec3(1.0) - blend, blend, corner);
                vec3 slopes = derivative * (corner * 2.0 - 1.0);
                float value = lattice_noise(cell + ivec3(x, y, z), seed);
                gradient += value * vec3(slopes.x * weights.y * weights.z,
                    weights.x * slopes.y * weights.z, weights.x * weights.y * slopes.z);
            }
        }
    }
    return gradient;
}

vec3 curl_noise(vec3 position, uint seed, int octaves) {
    vec3 curl = vec3(0.0);
    float weight = 1.0;
    float total_weight = 0.0;
    for (int octave = 0; octave < octaves; ++octave) {
        uint octave_seed = hash_u32(seed + uint(octave) * 0x9e3779b9u);
        vec3 dx = noise_gradient(position, octave_seed);
        vec3 dy = noise_gradient(position + vec3(37.1, 17.7, 53.3), octave_seed ^ 0x68bc21ebu);
        vec3 dz = noise_gradient(position + vec3(13.7, 71.3, 29.1), octave_seed ^ 0x02e5be93u);
        curl += weight * vec3(dz.y - dy.z, dx.z - dz.x, dy.x - dx.y);
        total_weight += weight;
        weight *= 0.5;
        position *= 2.0;
    }
    return curl / total_weight;
}

void apply_forces(vec3 position, inout vec3 velocity, uint step) {
    for (int index = 0; index < uForceCount; ++index) {
        vec4 first = uForceFirst[index];
        vec4 second = uForceSecond[index];
        int kind = uForceKinds[index];
        if (kind == 0) {
            velocity += first.xyz * STEP_SECONDS;
        } else if (kind == 1) {
            velocity /= 1.0 + first.x * STEP_SECONDS;
        } else if (kind == 2 && first.x != 0.0) {
            float time = float(step) * STEP_SECONDS * first.z;
            vec3 field_position = position * first.y + vec3(time * 0.73, time * 0.37, time * 0.53);
            // The offset also prevents a point emitter at the origin from
            // being trapped exactly at the zero derivative of a noise lattice.
            uint seed = hash_u32(uSeed ^ uForceSeeds[index]);
            vec3 offset = vec3(float(seed & 255u), float((seed >> 8) & 255u), float((seed >> 16) & 255u)) + vec3(0.371, 0.613, 0.193);
            velocity += curl_noise(field_position + offset, seed, int(first.w)) * (first.x * STEP_SECONDS);
        } else if (kind == 3 && first.w != 0.0) {
            vec3 axis = normalize(first.xyz);
            vec3 radial = position - second.xyz;
            vec3 tangent = cross(axis, radial);
            float tangent_length = length(tangent);
            if (tangent_length > 0.000001) {
                velocity += (tangent / tangent_length) * (first.w * STEP_SECONDS);
            }
        } else if (kind == 4 && first.w != 0.0) {
            vec3 delta = first.xyz - position;
            float distance_to_target = length(delta);
            if (distance_to_target > 0.000001 && distance_to_target < second.x) {
                float attenuation = pow(1.0 - distance_to_target / second.x, second.y);
                velocity += (delta / distance_to_target) * (first.w * attenuation * STEP_SECONDS);
            }
        }
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;

    #[test]
    fn tiny_vortex_axes_are_normalized_before_the_gpu_float_boundary() {
        let axis = crate::model::property::Vec3 {
            x: OrderedFloat(1e-100),
            y: OrderedFloat(0.0),
            z: OrderedFloat(0.0),
        };
        assert_eq!(
            normalized_direction(axis, "Particle vortex axis").unwrap(),
            [1.0, 0.0, 0.0]
        );
        let zero = crate::model::property::Vec3 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
            z: OrderedFloat(0.0),
        };
        assert!(normalized_direction(zero, "Particle vortex axis").is_err());
    }

    #[test]
    fn force_encoding_keeps_authored_order_and_full_seed_bits() {
        let encoded = ForceUniformData::new(&[
            ParticleForce::Drag {
                coefficient: OrderedFloat(0.25),
            },
            ParticleForce::Turbulence {
                strength: OrderedFloat(120.0),
                frequency: OrderedFloat(0.01),
                octaves: 4,
                evolution: OrderedFloat(2.0),
                seed: u32::MAX,
            },
            ParticleForce::Drag {
                coefficient: OrderedFloat(3.0),
            },
        ])
        .unwrap();
        assert_eq!(encoded.count, 3);
        assert_eq!(&encoded.kinds[..3], &[1, 2, 1]);
        assert_eq!(encoded.seeds[1], u32::MAX);
        assert_eq!(&encoded.first[4..8], &[120.0, 0.01, 2.0, 4.0]);
        assert_eq!(encoded.first[8], 3.0);
    }

    #[test]
    fn force_encoding_rejects_overflow_instead_of_truncating_the_stack() {
        let force = ParticleForce::Drag {
            coefficient: OrderedFloat(1.0),
        };
        assert!(ForceUniformData::new(&vec![force.clone(); PARTICLE_MAX_FORCES]).is_ok());
        assert!(ForceUniformData::new(&vec![force; PARTICLE_MAX_FORCES + 1]).is_err());
    }
}
