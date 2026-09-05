//! Typed Particle node contracts.
//!
//! Only the bounded Emitter -> Emitter Shape -> Birth Attributes -> Gravity
//! -> Drag -> Sprite slice has a native runtime. Every other descriptor
//! remains explicitly disabled.

use ordered_float::OrderedFloat;

use super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::frame::color::Color;
use crate::model::frame::particle::{
    validate_particle_cold_replay_budget, validate_particle_size_range,
};
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};
use crate::model::property::{
    PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue, Vec3,
};

pub(crate) const PARTICLE_SYSTEM_PORT: &str = "particles";
pub(crate) const PARTICLE_SPRITE_RENDERER_CATALOG_ID: &str =
    ParticleNodeRole::SpriteRenderer.catalog_id();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParticleNodeRole {
    Emitter,
    ShapeLocation,
    Initialize,
    Gravity,
    Drag,
    SpriteRenderer,
}

impl ParticleNodeRole {
    pub(crate) const fn catalog_id(self) -> &'static str {
        match self {
            Self::Emitter => "native.particle.emitter",
            Self::ShapeLocation => "native.particle.shape-location",
            Self::Initialize => "native.particle.initialize",
            Self::Gravity => "native.particle.gravity-force",
            Self::Drag => "native.particle.drag-force",
            Self::SpriteRenderer => "native.particle.sprite-renderer",
        }
    }

    pub(crate) const fn execution_rank(self) -> u8 {
        match self {
            Self::Emitter => 0,
            Self::ShapeLocation => 1,
            Self::Initialize => 2,
            Self::Gravity => 3,
            Self::Drag => 4,
            Self::SpriteRenderer => 5,
        }
    }

    pub(crate) fn from_catalog_id(catalog_id: &str) -> Option<Self> {
        [
            Self::Emitter,
            Self::ShapeLocation,
            Self::Initialize,
            Self::Gravity,
            Self::Drag,
            Self::SpriteRenderer,
        ]
        .into_iter()
        .find(|role| role.catalog_id() == catalog_id)
    }
}

const PARTICLE: PortSpec = PortSpec::single(
    PARTICLE_SYSTEM_PORT,
    "Particles",
    PortDataType::ParticleSystem,
);
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
    PortSpec::single("shape", "Shape", PortDataType::String),
    PortSpec::single("position", "Position", PortDataType::Vec3),
    PortSpec::single("radius", "Radius", PortDataType::Number),
    PortSpec::single("size", "Size", PortDataType::Vec3),
    PortSpec::single("surface_only", "Surface Only", PortDataType::Boolean),
];
const INITIALIZE_PARTICLE_INPUTS: &[PortSpec] = &[
    PARTICLE,
    PortSpec::single("velocity_min", "Velocity Min", PortDataType::Vec3),
    PortSpec::single("velocity_max", "Velocity Max", PortDataType::Vec3),
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
const PARTICLE_FIXED_STEP_REASON: &str = "deterministic Particle simulation needs a fixed-step parameter schedule, which is not implemented yet";
const EMITTER_CONSTANT_ONLY_INPUTS: &[&str] = &["capacity", "rate", "lifetime", "seed"];
const INITIALIZE_CONSTANT_ONLY_INPUTS: &[&str] =
    &["velocity_min", "velocity_max", "size_min", "size_max"];
const SHAPE_LOCATION_CONSTANT_ONLY_INPUTS: &[&str] =
    &["shape", "position", "radius", "size", "surface_only"];
const GRAVITY_CONSTANT_ONLY_INPUTS: &[&str] = &["force"];
const DRAG_CONSTANT_ONLY_INPUTS: &[&str] = &["coefficient"];
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
            ParticleNodeRole::Emitter.catalog_id(),
            "Particle Emitter",
            "Particles",
            "node_editor.menu.create.particle_emitter",
            &["particle", "emitter", "gpu", "rate", "seed"],
        ),
        PARTICLE_EMITTER_INPUTS,
        PARTICLE_OUTPUT,
        emitter_properties,
    )
    .validate_property_set(validate_emitter_property_set)
    .constant_only_inputs(EMITTER_CONSTANT_ONLY_INPUTS, PARTICLE_FIXED_STEP_REASON),
    DescriptorSpec::placeholder(
        "native.particle.spawn-burst",
        "Spawn Burst",
        "Particles",
        SPAWN_BURST_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            ParticleNodeRole::ShapeLocation.catalog_id(),
            "Emitter Shape",
            "Particles",
            "node_editor.menu.create.particle_shape_location",
            &["particle", "emitter", "spawn", "point", "box", "sphere"],
        ),
        SHAPE_LOCATION_INPUTS,
        PARTICLE_OUTPUT,
        shape_location_properties,
    )
    .validate_property_set(validate_shape_location_property_set)
    .constant_only_inputs(
        SHAPE_LOCATION_CONSTANT_ONLY_INPUTS,
        PARTICLE_FIXED_STEP_REASON,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            ParticleNodeRole::Initialize.catalog_id(),
            "Birth Attributes",
            "Particles",
            "node_editor.menu.create.particle_initialize",
            &["particle", "initialize", "velocity", "size"],
        ),
        INITIALIZE_PARTICLE_INPUTS,
        PARTICLE_OUTPUT,
        initialize_properties,
    )
    .validate_property_set(validate_initialize_property_set)
    .constant_only_inputs(INITIALIZE_CONSTANT_ONLY_INPUTS, PARTICLE_FIXED_STEP_REASON),
    DescriptorSpec::placeholder(
        "native.particle.set-attribute",
        "Set Attribute",
        "Particles",
        SET_ATTRIBUTE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            ParticleNodeRole::Gravity.catalog_id(),
            "Gravity Force",
            "Particles",
            "node_editor.menu.create.particle_gravity",
            &["particle", "gravity", "force", "gpu"],
        ),
        GRAVITY_INPUTS,
        PARTICLE_OUTPUT,
        gravity_properties,
    )
    .constant_only_inputs(GRAVITY_CONSTANT_ONLY_INPUTS, PARTICLE_FIXED_STEP_REASON),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            ParticleNodeRole::Drag.catalog_id(),
            "Drag Force",
            "Particles",
            "node_editor.menu.create.particle_drag",
            &["particle", "drag", "force", "gpu"],
        ),
        DRAG_INPUTS,
        PARTICLE_OUTPUT,
        drag_properties,
    )
    .constant_only_inputs(DRAG_CONSTANT_ONLY_INPUTS, PARTICLE_FIXED_STEP_REASON),
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
            ParticleNodeRole::SpriteRenderer.catalog_id(),
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

fn validate_emitter_property_set(properties: &PropertyMap) -> Result<(), String> {
    let capacity = match properties
        .get("capacity")
        .and_then(|property| property.value())
    {
        Some(PropertyValue::Integer(value)) => u32::try_from(*value)
            .map_err(|_| "Particle capacity does not fit the runtime range".to_string())?,
        _ => return Err("Particle Emitter requires an integer 'capacity' Property".to_string()),
    };
    let lifetime = match properties
        .get("lifetime")
        .and_then(|property| property.value())
    {
        Some(PropertyValue::Number(value)) => value.into_inner(),
        _ => return Err("Particle Emitter requires a numeric 'lifetime' Property".to_string()),
    };
    validate_particle_cold_replay_budget(capacity, lifetime)
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

fn shape_location_properties() -> Vec<PropertyDefinition> {
    vec![
        PropertyDefinition::new(
            "shape",
            PropertyUiType::Dropdown {
                options: vec!["Point".to_string(), "Box".to_string(), "Sphere".to_string()],
            },
            "Shape",
            PropertyValue::String("Point".to_string()),
        ),
        vec3_property("position", "Position", [0.0, 0.0, 0.0], " px"),
        number_property("radius", "Radius", 0.0, 1_000_000.0, 100.0, " px"),
        vec3_property("size", "Size", [200.0, 200.0, 200.0], " px"),
        PropertyDefinition::new(
            "surface_only",
            PropertyUiType::Bool,
            "Surface Only",
            PropertyValue::Boolean(false),
        ),
    ]
}

fn validate_shape_location_property_set(properties: &PropertyMap) -> Result<(), String> {
    let shape = match properties
        .get("shape")
        .and_then(|property| property.value())
    {
        Some(PropertyValue::String(value)) => value.as_str(),
        _ => return Err("Emitter Shape requires a string 'shape' Property".to_string()),
    };
    if !matches!(shape, "Point" | "Box" | "Sphere") {
        return Err(format!("Emitter Shape has unknown shape '{shape}'"));
    }
    let size = match properties.get("size").and_then(|property| property.value()) {
        Some(PropertyValue::Vec3(value)) => value,
        _ => return Err("Emitter Shape requires a Vec3 'size' Property".to_string()),
    };
    if [size.x, size.y, size.z]
        .into_iter()
        .any(|component| component.into_inner() < 0.0)
    {
        return Err("Emitter Shape size components must be non-negative".to_string());
    }
    Ok(())
}

fn validate_initialize_property_set(properties: &PropertyMap) -> Result<(), String> {
    let number = |key: &str| match properties.get(key).and_then(|property| property.value()) {
        Some(PropertyValue::Number(value)) => Ok(value.into_inner()),
        _ => Err(format!(
            "Birth Attributes requires a numeric '{key}' Property"
        )),
    };
    validate_particle_size_range(number("size_min")?, number("size_max")?)
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
