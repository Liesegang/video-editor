//! Producer-specific access behind one logical render-Point shader contract.

use glow::HasContext;

use crate::error::LibraryError;
use crate::model::frame::point::{POINT_GRID_AXIS_BITS, PointGridParameters, PointSceneSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum PointSourceKind {
    Particle,
    Grid,
}

impl PointSourceKind {
    pub fn of(source: &PointSceneSource) -> Self {
        match source {
            PointSceneSource::Particle { .. } => Self::Particle,
            PointSceneSource::Grid(_) => Self::Grid,
        }
    }

    pub fn shader(self) -> String {
        match self {
            Self::Particle => PARTICLE_POINT_SOURCE.to_string(),
            Self::Grid => GRID_POINT_SOURCE
                .replace("POINT_GRID_AXIS_BITS", &format!("{POINT_GRID_AXIS_BITS}u")),
        }
    }
}

pub(super) enum PointSourceBinding<'a> {
    Particle { buffer: glow::Buffer },
    Grid(&'a PointGridParameters),
}

impl PointSourceBinding<'_> {
    pub fn kind(&self) -> PointSourceKind {
        match self {
            Self::Particle { .. } => PointSourceKind::Particle,
            Self::Grid(_) => PointSourceKind::Grid,
        }
    }

    pub fn bind(
        &self,
        gl: &glow::Context,
        uniforms: &PointSourceUniforms,
    ) -> Result<(), LibraryError> {
        match (self, uniforms) {
            (Self::Particle { buffer }, PointSourceUniforms::Particle) => {
                // SAFETY: the buffer is owned by this SceneRuntime and the
                // source access declares the matching binding-zero ABI.
                unsafe {
                    gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(*buffer));
                }
                Ok(())
            }
            (Self::Grid(parameters), PointSourceUniforms::Grid(uniforms)) => {
                let spacing = vec3_f32(parameters.spacing, "Grid spacing")?;
                let center = vec3_f32(parameters.center, "Grid center")?;
                // SAFETY: all locations belong to the currently selected
                // source program; model validation bounds counts and size.
                unsafe {
                    gl.uniform_3_u32(
                        uniforms.counts.as_ref(),
                        parameters.counts[0],
                        parameters.counts[1],
                        parameters.counts[2],
                    );
                    gl.uniform_3_f32(
                        uniforms.spacing.as_ref(),
                        spacing[0],
                        spacing[1],
                        spacing[2],
                    );
                    gl.uniform_3_f32(uniforms.center.as_ref(), center[0], center[1], center[2]);
                    gl.uniform_1_f32(uniforms.size.as_ref(), parameters.size.into_inner());
                }
                Ok(())
            }
            _ => Err(LibraryError::Validation(
                "Point source changed without rebuilding its GPU access".to_string(),
            )),
        }
    }
}

#[derive(Clone)]
pub(super) enum PointSourceUniforms {
    Particle,
    Grid(GridUniforms),
}

#[derive(Clone)]
pub(super) struct GridUniforms {
    counts: Option<glow::UniformLocation>,
    spacing: Option<glow::UniformLocation>,
    center: Option<glow::UniformLocation>,
    size: Option<glow::UniformLocation>,
}

#[derive(Clone, Copy)]
pub(super) enum PointSourceRequirements {
    Optional,
    SizeAndAlive,
    FullGeometry,
}

impl PointSourceUniforms {
    pub fn new(
        gl: &glow::Context,
        program: glow::Program,
        kind: PointSourceKind,
        requirements: PointSourceRequirements,
    ) -> Result<Self, LibraryError> {
        let uniforms = match kind {
            PointSourceKind::Particle => Self::Particle,
            PointSourceKind::Grid => Self::Grid(GridUniforms {
                // A field-only shader may optimize geometry uniforms away;
                // the shared Sprite shader consumes every one.
                counts: uniform(gl, program, "uGridCounts"),
                spacing: uniform(gl, program, "uGridSpacing"),
                center: uniform(gl, program, "uGridCenter"),
                size: uniform(gl, program, "uGridSize"),
            }),
        };
        if let Self::Grid(grid) = &uniforms {
            let missing = match requirements {
                PointSourceRequirements::Optional => false,
                PointSourceRequirements::SizeAndAlive => {
                    grid.counts.is_none() || grid.size.is_none()
                }
                PointSourceRequirements::FullGeometry => [
                    grid.counts.as_ref(),
                    grid.spacing.as_ref(),
                    grid.center.as_ref(),
                    grid.size.as_ref(),
                ]
                .iter()
                .any(|location| location.is_none()),
            };
            if missing {
                return Err(LibraryError::Render(
                    "GPU Point Grid sprite omitted required source uniforms".to_string(),
                ));
            }
        }
        Ok(uniforms)
    }
}

fn uniform(
    gl: &glow::Context,
    program: glow::Program,
    name: &str,
) -> Option<glow::UniformLocation> {
    // SAFETY: the program is linked and belongs to this current context.
    unsafe { gl.get_uniform_location(program, name) }
}

fn vec3_f32(value: crate::model::property::Vec3, label: &str) -> Result<[f32; 3], LibraryError> {
    let value = [
        value.x.into_inner() as f32,
        value.y.into_inner() as f32,
        value.z.into_inner() as f32,
    ];
    value
        .iter()
        .all(|component| component.is_finite())
        .then_some(value)
        .ok_or_else(|| LibraryError::Validation(format!("{label} must fit finite GPU floats")))
}

pub(super) const RENDER_POINT_GLSL: &str = r#"
struct RenderPoint {
    vec4 position_size;
    uint serial;
    bool alive;
};
"#;

const PARTICLE_POINT_SOURCE: &str = r#"
// PARTICLE_STRUCT
layout(std430, binding = 0) readonly buffer ParticleBuffer {
    Particle particles[];
};

RenderPoint load_render_point(uint slot) {
    Particle particle = particles[slot];
    float age = particle.position_age.w;
    bool alive = age >= 0.0 && age < particle.velocity_lifetime.w;
    return RenderPoint(
        vec4(particle.position_age.xyz, particle.appearance.w),
        particle.identity.x,
        alive
    );
}

bool load_point_age(uint slot, out float value) {
    value = particles[slot].position_age.w;
    return true;
}

bool load_point_normalized_age(uint slot, out float value) {
    Particle particle = particles[slot];
    value = clamp(particle.position_age.w / particle.velocity_lifetime.w, 0.0, 1.0);
    return particle.velocity_lifetime.w > 0.0;
}
"#;

const GRID_POINT_SOURCE: &str = r#"
uniform uvec3 uGridCounts;
uniform vec3 uGridSpacing;
uniform vec3 uGridCenter;
uniform float uGridSize;

RenderPoint load_render_point(uint slot) {
    uint xy = uGridCounts.x * uGridCounts.y;
    uint z = slot / xy;
    uint remainder = slot - z * xy;
    uint y = remainder / uGridCounts.x;
    uint x = remainder - y * uGridCounts.x;
    vec3 coordinate = vec3(x, y, z);
    vec3 centered = coordinate - (vec3(uGridCounts) - vec3(1.0)) * 0.5;
    uint serial = x | (y << POINT_GRID_AXIS_BITS) | (z << (2u * POINT_GRID_AXIS_BITS));
    return RenderPoint(
        vec4(uGridCenter + centered * uGridSpacing, uGridSize),
        serial,
        slot < uGridCounts.x * uGridCounts.y * uGridCounts.z
    );
}

"#;
