//! Trusted built-in GPU Particle kernels.
//!
//! Authored strings never enter these sources. Module graph compilation only
//! selects this fixed ABI and supplies validated uniforms.

pub(super) fn particle_compute_source() -> String {
    PARTICLE_COMPUTE
        .replace("// PARTICLE_STRUCT", PARTICLE_STRUCT_GLSL)
        .replacen(
            "#version 430 core",
            &format!(
                "#version 430 core\n#define MAX_PARTICLE_FORCES {}",
                crate::model::frame::particle::PARTICLE_MAX_FORCES
            ),
            1,
        )
        .replace("// PARTICLE_RANDOM_FUNCTIONS", PARTICLE_RANDOM_FUNCTIONS)
        .replace("// PARTICLE_FORCE_FUNCTIONS", super::forces::FORCE_GLSL)
}

pub(super) const PARTICLE_STRUCT_GLSL: &str = r#"
struct Particle {
    vec4 position_age;
    vec4 velocity_lifetime;
    vec4 appearance;
    uvec4 identity;
};
"#;

pub(super) const PARTICLE_RANDOM_FUNCTIONS: &str = r#"
uint hash_u32(uint value) {
    value ^= value >> 16;
    value *= 0x7feb352du;
    value ^= value >> 15;
    value *= 0x846ca68bu;
    return value ^ (value >> 16);
}

float random_01(uint seed, uint serial, uint channel) {
    uint bits = hash_u32(seed ^ hash_u32(serial + channel * 0x9e3779b9u));
    return float(bits & 0x00ffffffu) / 16777216.0;
}
"#;

const PARTICLE_COMPUTE: &str = r#"#version 430 core
layout(local_size_x = 64) in;

// PARTICLE_STRUCT

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
uniform int uEmitterShape;
uniform vec3 uEmitterPosition;
uniform float uEmitterRadius;
uniform vec3 uEmitterSize;
uniform bool uEmitterSurfaceOnly;
uniform vec3 uVelocityMin;
uniform vec3 uVelocityMax;
uniform float uSizeMin;
uniform float uSizeMax;

const float STEP_SECONDS = 1.0 / 120.0;

// PARTICLE_RANDOM_FUNCTIONS

vec3 sphere_direction(uint serial) {
    float z = random_01(uSeed, serial, 7u) * 2.0 - 1.0;
    float angle = random_01(uSeed, serial, 8u) * 6.28318530718;
    float radial = sqrt(max(0.0, 1.0 - z * z));
    return vec3(radial * cos(angle), radial * sin(angle), z);
}

vec3 box_position(uint serial) {
    vec3 normalized = vec3(
        random_01(uSeed, serial, 7u),
        random_01(uSeed, serial, 8u),
        random_01(uSeed, serial, 9u)
    ) - vec3(0.5);
    if (uEmitterSurfaceOnly) {
        int face = min(5, int(floor(random_01(uSeed, serial, 10u) * 6.0)));
        int axis = face / 2;
        normalized[axis] = (face % 2 == 0) ? -0.5 : 0.5;
    }
    return normalized * uEmitterSize;
}

vec3 emitter_position(uint serial) {
    if (uEmitterShape == 1) {
        return uEmitterPosition + box_position(serial);
    }
    if (uEmitterShape == 2) {
        float distance_from_center = uEmitterSurfaceOnly
            ? uEmitterRadius
            : uEmitterRadius * pow(random_01(uSeed, serial, 9u), 1.0 / 3.0);
        return uEmitterPosition + sphere_direction(serial) * distance_from_center;
    }
    return uEmitterPosition;
}

// PARTICLE_FORCE_FUNCTIONS

void spawn(inout Particle particle, uint serial) {
    vec3 random_velocity = vec3(
        random_01(uSeed, serial, 0u),
        random_01(uSeed, serial, 1u),
        random_01(uSeed, serial, 2u)
    );
    particle.position_age = vec4(emitter_position(serial), 0.0);
    particle.velocity_lifetime = vec4(
        mix(uVelocityMin, uVelocityMax, random_velocity),
        uLifetime
    );
    particle.appearance = vec4(
        random_01(uSeed, serial, 3u),
        random_01(uSeed, serial, 4u),
        random_01(uSeed, serial, 5u),
        mix(uSizeMin, uSizeMax, random_01(uSeed, serial, 6u))
    );
    particle.identity = uvec4(serial, 0u, 0u, 0u);
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
        particles[slot].identity = uvec4(0u);
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
                apply_forces(particle.position_age.xyz, particle.velocity_lifetime.xyz, step);
                particle.position_age.xyz += particle.velocity_lifetime.xyz * STEP_SECONDS;
            }
        }
    }
    particles[slot] = particle;
}
"#;

pub(super) fn particle_vertex_source(point_fields: bool) -> String {
    PARTICLE_VERTEX
        .replace("// PARTICLE_STRUCT", PARTICLE_STRUCT_GLSL)
        .replace(
            "// POINT_COLOR_BUFFER",
            if point_fields { POINT_COLOR_BUFFER } else { "" },
        )
        .replace(
            "// POINT_COLOR_OUTPUT",
            if point_fields {
                "out vec4 vLinearColor;"
            } else {
                ""
            },
        )
        .replace(
            "// POINT_COLOR_HIDDEN",
            if point_fields {
                "vLinearColor = vec4(0.0);"
            } else {
                ""
            },
        )
        .replace(
            "// POINT_COLOR_ASSIGN",
            if point_fields {
                "vLinearColor = pointColors[particle_index];"
            } else {
                ""
            },
        )
}

const PARTICLE_VERTEX: &str = r#"#version 430 core
// PARTICLE_STRUCT

layout(std430, binding = 0) readonly buffer ParticleBuffer {
    Particle particles[];
};
// POINT_COLOR_BUFFER

uniform vec2 uLogicalSize;
uniform vec2 uTargetSize;
uniform vec3 uAffineX;
uniform vec3 uAffineY;
uniform float uFocalLength;

out vec2 vSpriteCoord;
// POINT_COLOR_OUTPUT

const vec2 QUAD_CORNERS[6] = vec2[6](
    vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(0.5, 0.5),
    vec2(-0.5, -0.5), vec2(0.5, 0.5), vec2(-0.5, 0.5)
);

void main() {
    uint particle_index = uint(gl_VertexID) / 6u;
    uint corner_index = uint(gl_VertexID) % 6u;
    Particle particle = particles[particle_index];
    float age = particle.position_age.w;
    if (age < 0.0 || age >= particle.velocity_lifetime.w) {
        gl_Position = vec4(2.0, 2.0, 1.0, 1.0);
        vSpriteCoord = vec2(-1.0);
        // POINT_COLOR_HIDDEN
        return;
    }

    vec3 position = particle.position_age.xyz;
    float perspective = uFocalLength / max(1.0, uFocalLength + position.z);
    vec2 corner = QUAD_CORNERS[corner_index];
    vec2 local_center = uLogicalSize * 0.5 + position.xy * perspective;
    vec2 local = local_center + corner * particle.appearance.w * perspective;
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
    vSpriteCoord = corner + vec2(0.5);
    // POINT_COLOR_ASSIGN
}
"#;

pub(super) fn particle_fragment_source(point_fields: bool) -> String {
    PARTICLE_FRAGMENT
        .replace(
            "// COLOR_UNIFORMS",
            if point_fields {
                "uniform bool uOutputSrgba;\nin vec4 vLinearColor;"
            } else {
                "uniform vec4 uPremultipliedColor;"
            },
        )
        .replace(
            "// COLOR_FUNCTION",
            if point_fields { LINEAR_TO_SRGB } else { "" },
        )
        .replace(
            "// COLOR_OUTPUT",
            if point_fields {
                POINT_COLOR_OUTPUT
            } else {
                "output_color = uPremultipliedColor * coverage;"
            },
        )
}

const PARTICLE_FRAGMENT: &str = r#"#version 430 core
// COLOR_UNIFORMS
in vec2 vSpriteCoord;
layout(location = 0) out vec4 output_color;
// COLOR_FUNCTION

void main() {
    float radius = length(vSpriteCoord - vec2(0.5)) * 2.0;
    float coverage = 1.0 - smoothstep(0.82, 1.0, radius);
    if (coverage <= 0.0) {
        discard;
    }
    // COLOR_OUTPUT
}
"#;
const POINT_COLOR_BUFFER: &str = r#"layout(std430, binding = 2) readonly buffer PointColorBuffer {
    vec4 pointColors[];
};"#;

const LINEAR_TO_SRGB: &str = r#"
float linear_to_srgb(float value) {
    return value <= 0.0031308
        ? value * 12.92
        : 1.055 * pow(max(value, 0.0), 1.0 / 2.4) - 0.055;
}
"#;

const POINT_COLOR_OUTPUT: &str = r#"
    if (vLinearColor.a <= 0.0) discard;
    vec3 rgb = uOutputSrgba
        ? vec3(
            linear_to_srgb(vLinearColor.r),
            linear_to_srgb(vLinearColor.g),
            linear_to_srgb(vLinearColor.b)
        )
        : vLinearColor.rgb;
    output_color = vec4(rgb * vLinearColor.a, vLinearColor.a) * coverage;
"#;
