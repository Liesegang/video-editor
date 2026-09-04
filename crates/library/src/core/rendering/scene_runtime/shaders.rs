//! Trusted built-in GPU Particle kernels.
//!
//! Authored strings never enter these sources. Module graph compilation only
//! selects this fixed ABI and supplies validated uniforms.

pub(super) const PARTICLE_COMPUTE: &str = r#"#version 430 core
layout(local_size_x = 64) in;

struct Particle {
    vec4 position_age;
    vec4 velocity_lifetime;
    vec4 appearance;
};

layout(std430, binding = 0) buffer ParticleBuffer {
    Particle particles[];
};

uniform uint uCapacity;
uniform bool uReset;
uniform uint uSeed;
uniform uint uStartStep;
uniform uint uStepCount;
uniform float uRate;
uniform float uLifetime;
uniform vec3 uVelocityMin;
uniform vec3 uVelocityMax;
uniform vec3 uGravity;
uniform float uDrag;
uniform float uSizeMin;
uniform float uSizeMax;

const float STEP_SECONDS = 1.0 / 120.0;

uint hash_u32(uint value) {
    value ^= value >> 16;
    value *= 0x7feb352du;
    value ^= value >> 15;
    value *= 0x846ca68bu;
    return value ^ (value >> 16);
}

float random_01(uint serial, uint channel) {
    uint bits = hash_u32(uSeed ^ hash_u32(serial + channel * 0x9e3779b9u));
    return float(bits & 0x00ffffffu) / 16777216.0;
}

void spawn(inout Particle particle, uint serial) {
    vec3 random_velocity = vec3(
        random_01(serial, 0u),
        random_01(serial, 1u),
        random_01(serial, 2u)
    );
    particle.position_age = vec4(0.0, 0.0, 0.0, 0.0);
    particle.velocity_lifetime = vec4(
        mix(uVelocityMin, uVelocityMax, random_velocity),
        uLifetime
    );
    particle.appearance = vec4(
        random_01(serial, 3u),
        random_01(serial, 4u),
        random_01(serial, 5u),
        mix(uSizeMin, uSizeMax, random_01(serial, 6u))
    );
}

void main() {
    uint slot = gl_GlobalInvocationID.x;
    if (slot >= uCapacity) {
        return;
    }

    if (uReset) {
        particles[slot].position_age = vec4(0.0, 0.0, 0.0, -1.0);
        particles[slot].velocity_lifetime = vec4(0.0);
        particles[slot].appearance = vec4(0.0);
        return;
    }

    Particle particle = particles[slot];
    for (uint offset = 0u; offset < uStepCount; ++offset) {
        uint step = uStartStep + offset;
        uint emit_begin = uint(floor(float(step) * uRate * STEP_SECONDS));
        uint emit_end = uint(floor(float(step + 1u) * uRate * STEP_SECONDS));
        bool emitted = false;
        uint serial = 0u;
        if (emit_end > emit_begin) {
            uint latest = emit_end - 1u;
            uint latest_slot = latest % uCapacity;
            uint distance = (latest_slot + uCapacity - slot) % uCapacity;
            if (latest >= distance) {
                serial = latest - distance;
                emitted = serial >= emit_begin;
            }
        }

        if (emitted) {
            spawn(particle, serial);
        } else if (particle.position_age.w >= 0.0) {
            particle.position_age.w += STEP_SECONDS;
            if (particle.position_age.w >= particle.velocity_lifetime.w) {
                particle.position_age.w = -1.0;
            } else {
                particle.velocity_lifetime.xyz += uGravity * STEP_SECONDS;
                particle.velocity_lifetime.xyz /= 1.0 + uDrag * STEP_SECONDS;
                particle.position_age.xyz += particle.velocity_lifetime.xyz * STEP_SECONDS;
            }
        }
    }
    particles[slot] = particle;
}
"#;

pub(super) const PARTICLE_VERTEX: &str = r#"#version 430 core
struct Particle {
    vec4 position_age;
    vec4 velocity_lifetime;
    vec4 appearance;
};

layout(std430, binding = 0) readonly buffer ParticleBuffer {
    Particle particles[];
};

uniform vec2 uLogicalSize;
uniform vec2 uTargetSize;
uniform vec3 uAffineX;
uniform vec3 uAffineY;
uniform float uFocalLength;
uniform float uPointScale;

void main() {
    Particle particle = particles[gl_VertexID];
    float age = particle.position_age.w;
    if (age < 0.0 || age >= particle.velocity_lifetime.w) {
        gl_Position = vec4(2.0, 2.0, 1.0, 1.0);
        gl_PointSize = 0.0;
        return;
    }

    vec3 position = particle.position_age.xyz;
    float perspective = uFocalLength / max(1.0, uFocalLength + position.z);
    vec2 local = uLogicalSize * 0.5 + position.xy * perspective;
    vec2 screen = vec2(
        dot(uAffineX, vec3(local, 1.0)),
        dot(uAffineY, vec3(local, 1.0))
    );
    vec2 ndc = vec2(
        screen.x * 2.0 / uTargetSize.x - 1.0,
        1.0 - screen.y * 2.0 / uTargetSize.y
    );
    float depth = clamp(position.z / (uFocalLength * 4.0), -0.99, 0.99);
    gl_Position = vec4(ndc, depth, 1.0);
    gl_PointSize = max(1.0, particle.appearance.w * perspective * uPointScale);
}
"#;

pub(super) const PARTICLE_FRAGMENT: &str = r#"#version 430 core
uniform vec4 uPremultipliedColor;
layout(location = 0) out vec4 output_color;

void main() {
    float radius = length(gl_PointCoord - vec2(0.5)) * 2.0;
    float coverage = 1.0 - smoothstep(0.82, 1.0, radius);
    if (coverage <= 0.0) {
        discard;
    }
    output_color = uPremultipliedColor * coverage;
}
"#;
