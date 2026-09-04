//! Typed Particle node contracts.
//!
//! Only the bounded Emitter -> Initialize -> Gravity -> Drag -> Sprite slice
//! has a native runtime. Every other descriptor remains explicitly disabled.

use ordered_float::OrderedFloat;

use super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::frame::color::Color;
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec3};

const PARTICLE: PortSpec = PortSpec::single("particles", "Particles", PortDataType::ParticleSystem);
const PARTICLE_OUTPUT: &[PortSpec] = &[PARTICLE];
const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];

const PARTICLE_EMITTER_INPUTS: &[PortSpec] = &[
    PortSpec::single("capacity", "Capacity", PortDataType::Integer),
    PortSpec::single("rate", "Rate", PortDataType::Number),
    PortSpec::single("lifetime", "Lifetime", PortDataType::Number),
    PortSpec::single("seed", "Seed", PortDataType::Integer),
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
    PortSpec::single("color", "Color", PortDataType::Color),
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
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            "native.particle.emitter",
            "Particle Emitter",
            "Particles",
            "node_editor.menu.create.particle_emitter",
            &["particle", "emitter", "gpu", "rate", "seed"],
        ),
        PARTICLE_EMITTER_INPUTS,
        PARTICLE_OUTPUT,
        emitter_properties,
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
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            "native.particle.initialize",
            "Initialize Particle",
            "Particles",
            "node_editor.menu.create.particle_initialize",
            &["particle", "initialize", "velocity", "size"],
        ),
        INITIALIZE_PARTICLE_INPUTS,
        PARTICLE_OUTPUT,
        initialize_properties,
    ),
    DescriptorSpec::placeholder(
        "native.particle.set-attribute",
        "Set Attribute",
        "Particles",
        SET_ATTRIBUTE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            "native.particle.gravity-force",
            "Gravity Force",
            "Particles",
            "node_editor.menu.create.particle_gravity",
            &["particle", "gravity", "force", "gpu"],
        ),
        GRAVITY_INPUTS,
        PARTICLE_OUTPUT,
        gravity_properties,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            "native.particle.drag-force",
            "Drag Force",
            "Particles",
            "node_editor.menu.create.particle_drag",
            &["particle", "drag", "force", "gpu"],
        ),
        DRAG_INPUTS,
        PARTICLE_OUTPUT,
        drag_properties,
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
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            "native.particle.sprite-renderer",
            "Sprite Renderer",
            "Particles",
            "node_editor.menu.create.particle_sprite_renderer",
            &["particle", "sprite", "render", "gpu"],
        ),
        SPRITE_RENDERER_INPUTS,
        IMAGE_OUTPUT,
        sprite_properties,
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

fn emitter_properties() -> Vec<PropertyDefinition> {
    vec![
        integer_property("capacity", "Capacity", 1, 100_000, 8_192),
        number_property("rate", "Rate", 0.0, 100_000.0, 120.0, " /s"),
        number_property("lifetime", "Lifetime", 1.0 / 120.0, 120.0, 4.0, " s"),
        integer_property("seed", "Seed", 0, i64::from(u32::MAX), 1),
    ]
}

fn initialize_properties() -> Vec<PropertyDefinition> {
    vec![
        vec3_property(
            "velocity_min",
            "Velocity Min",
            [-120.0, -260.0, -40.0],
            " px/s",
        ),
        vec3_property(
            "velocity_max",
            "Velocity Max",
            [120.0, -140.0, 40.0],
            " px/s",
        ),
        number_property("size_min", "Size Min", 0.25, 512.0, 6.0, " px"),
        number_property("size_max", "Size Max", 0.25, 512.0, 18.0, " px"),
    ]
}

fn gravity_properties() -> Vec<PropertyDefinition> {
    vec![vec3_property("force", "Force", [0.0, 180.0, 0.0], " px/s²")]
}

fn drag_properties() -> Vec<PropertyDefinition> {
    vec![number_property(
        "coefficient",
        "Coefficient",
        0.0,
        100.0,
        0.15,
        "",
    )]
}

fn sprite_properties() -> Vec<PropertyDefinition> {
    vec![PropertyDefinition::new(
        "color",
        PropertyUiType::Color,
        "Color",
        PropertyValue::Color(Color {
            r: 115,
            g: 205,
            b: 255,
            a: 220,
        }),
    )]
}

fn integer_property(
    name: &str,
    label: &str,
    min: i64,
    max: i64,
    default: i64,
) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Integer {
            min,
            max,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        label,
        PropertyValue::Integer(default),
    )
}

fn number_property(
    name: &str,
    label: &str,
    min: f64,
    max: f64,
    default: f64,
    suffix: &str,
) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Float {
            min,
            max,
            step: 0.1,
            suffix: suffix.to_string(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        label,
        PropertyValue::Number(OrderedFloat(default)),
    )
}

fn vec3_property(name: &str, label: &str, value: [f64; 3], suffix: &str) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::vec3_with_range(-1_000_000.0, 1_000_000.0, 0.1, suffix, true, true),
        label,
        PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(value[0]),
            y: OrderedFloat(value[1]),
            z: OrderedFloat(value[2]),
        }),
    )
}
