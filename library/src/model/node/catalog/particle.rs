//! Typed Particle node contracts whose runtime and renderer are still design-needed.

use super::descriptor::{DescriptorSpec, PortSpec};
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};

const PARTICLE: PortSpec = PortSpec::single("particles", "Particles", PortDataType::ParticleSystem);
const PARTICLE_OUTPUT: &[PortSpec] = &[PARTICLE];
const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];

const PARTICLE_EMITTER_INPUTS: &[PortSpec] = &[
    PortSpec::single("capacity", "Capacity", PortDataType::Integer),
    PortSpec::single("simulation_space", "Simulation Space", PortDataType::Enum),
    PortSpec::single("rate", "Rate", PortDataType::Number),
    PortSpec::single("lifetime", "Lifetime", PortDataType::Number),
    PortSpec::single("loop", "Loop", PortDataType::Boolean),
    PortSpec::single("duration", "Duration", PortDataType::Number),
];
const SPAWN_BURST_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("count", "Count", PortDataType::Integer),
    PortSpec::single("time", "Time", PortDataType::Number),
];
const SHAPE_LOCATION_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("shape", "Shape", PortDataType::Enum),
    PortSpec::single("radius", "Radius", PortDataType::Number),
    PortSpec::single("size", "Size", PortDataType::Vec3),
    PortSpec::single("surface_only", "Surface Only", PortDataType::Boolean),
];
const INITIALIZE_PARTICLE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("velocity_min", "Velocity Min", PortDataType::Vec3),
    PortSpec::single("velocity_max", "Velocity Max", PortDataType::Vec3),
    PortSpec::single("color_min", "Color Min", PortDataType::Color),
    PortSpec::single("color_max", "Color Max", PortDataType::Color),
    PortSpec::single("size_min", "Size Min", PortDataType::Number),
    PortSpec::single("size_max", "Size Max", PortDataType::Number),
];
const SET_ATTRIBUTE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("attribute_name", "Attribute Name", PortDataType::String),
    PortSpec::single("value", "Value", PortDataType::Any),
];
const GRAVITY_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("force", "Force", PortDataType::Vec3),
];
const DRAG_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("coefficient", "Coefficient", PortDataType::Number),
];
const POINT_FORCE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("target", "Target", PortDataType::Vec3),
    PortSpec::single("strength", "Strength", PortDataType::Number),
    PortSpec::single("radius", "Radius", PortDataType::Number),
    PortSpec::single("falloff", "Falloff", PortDataType::Number),
];
const VORTEX_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("axis", "Axis", PortDataType::Vec3),
    PortSpec::single("strength", "Strength", PortDataType::Number),
];
const VECTOR_FIELD_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("vector_field", "Vector Field", PortDataType::Asset),
    PortSpec::single("intensity", "Intensity", PortDataType::Number),
    PortSpec::single("tiling", "Tiling", PortDataType::Vec3),
];
const TURBULENCE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("frequency", "Frequency", PortDataType::Number),
    PortSpec::single("strength", "Strength", PortDataType::Number),
    PortSpec::single("octave", "Octave", PortDataType::Integer),
];
const COLOR_OVER_LIFE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("gradient", "Gradient", PortDataType::Gradient),
];
const SIZE_OVER_LIFE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("curve", "Curve", PortDataType::Curve),
];
const COLLISION_PLANE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("plane_point", "Plane Point", PortDataType::Vec3),
    PortSpec::single("plane_normal", "Plane Normal", PortDataType::Vec3),
    PortSpec::single("bounce", "Bounce", PortDataType::Number),
    PortSpec::single("friction", "Friction", PortDataType::Number),
];
const COLLISION_DEPTH_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("depth_buffer", "Depth Buffer", PortDataType::Image),
    PortSpec::single("thickness", "Thickness", PortDataType::Number),
];
const SPRITE_RENDERER_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("texture", "Texture", PortDataType::Image),
    PortSpec::single("color", "Color", PortDataType::Color),
    PortSpec::single("blend_mode", "Blend Mode", PortDataType::Enum),
    PortSpec::single("alignment", "Alignment", PortDataType::Enum),
];
const MESH_RENDERER_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("mesh", "Mesh", PortDataType::Asset),
    PortSpec::single("material", "Material", PortDataType::Material),
];
const RIBBON_RENDERER_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("texture", "Texture", PortDataType::Image),
    PortSpec::single("width", "Width", PortDataType::Number),
    PortSpec::single("max_trail_length", "Max Trail Length", PortDataType::Number),
];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::placeholder(
        "native.particle.emitter",
        "Particle Emitter",
        "Particles",
        PARTICLE_EMITTER_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.spawn-burst",
        "Spawn Burst",
        "Particles",
        SPAWN_BURST_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.shape-location",
        "Shape Location",
        "Particles",
        SHAPE_LOCATION_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.initialize",
        "Initialize Particle",
        "Particles",
        INITIALIZE_PARTICLE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.set-attribute",
        "Set Attribute",
        "Particles",
        SET_ATTRIBUTE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.gravity-force",
        "Gravity Force",
        "Particles",
        GRAVITY_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.drag-force",
        "Drag Force",
        "Particles",
        DRAG_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.point-force",
        "Point Force",
        "Particles",
        POINT_FORCE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.vortex-force",
        "Vortex Force",
        "Particles",
        VORTEX_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.vector-field-force",
        "Vector Field Force",
        "Particles",
        VECTOR_FIELD_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.turbulence",
        "Turbulence",
        "Particles",
        TURBULENCE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.color-over-life",
        "Color Over Life",
        "Particles",
        COLOR_OVER_LIFE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.size-over-life",
        "Size Over Life",
        "Particles",
        SIZE_OVER_LIFE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.collision-plane",
        "Collision Plane",
        "Particles",
        COLLISION_PLANE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.collision-depth",
        "Collision Depth",
        "Particles",
        COLLISION_DEPTH_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.sprite-renderer",
        "Sprite Renderer",
        "Particles",
        SPRITE_RENDERER_INPUTS,
        IMAGE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.mesh-renderer",
        "Mesh Renderer",
        "Particles",
        MESH_RENDERER_INPUTS,
        IMAGE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.particle.ribbon-renderer",
        "Ribbon Renderer",
        "Particles",
        RIBBON_RENDERER_INPUTS,
        IMAGE_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
