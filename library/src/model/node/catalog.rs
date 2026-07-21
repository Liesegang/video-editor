//! Central first-party Node catalog.
//!
//! Menu presentation, detached factories, QA identity, persisted placeholder
//! identity, and graph ports all consume these descriptors. Runtime-specific
//! implementations may remain elsewhere, but must keep this contract.

use std::sync::LazyLock;

use super::{GeneratorContent, NativeOperationContent, Node, NodeContent, ValueContent};
use crate::model::project::{
    FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortDataType,
    PortDefinition, PortExposure, PortMultiplicity, PortSide, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::PropertyMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeNodeRuntimeStatus {
    Implemented,
    DesignNeeded,
}

impl NativeNodeRuntimeStatus {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::DesignNeeded => "design-needed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeNodeFactory {
    Generator(GeneratorContent),
    Value(ValueContent),
    Merge,
    TypedPlaceholder,
}

#[derive(Clone, Debug)]
pub struct NativeNodeCatalogDescriptor {
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    qa_id: &'static str,
    keywords: &'static [&'static str],
    runtime_status: NativeNodeRuntimeStatus,
    factory: NativeNodeFactory,
    ports: Vec<PortDefinition>,
}

impl NativeNodeCatalogDescriptor {
    pub fn catalog_id(&self) -> &'static str {
        self.catalog_id
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn category(&self) -> &'static str {
        self.category
    }

    pub fn qa_id(&self) -> String {
        if self.runtime_status == NativeNodeRuntimeStatus::DesignNeeded {
            format!("node_editor.menu.create.catalog:{}", self.catalog_id)
        } else {
            self.qa_id.to_string()
        }
    }

    pub fn keywords(&self) -> &'static [&'static str] {
        self.keywords
    }

    pub fn runtime_status(&self) -> NativeNodeRuntimeStatus {
        self.runtime_status
    }

    pub fn factory(&self) -> NativeNodeFactory {
        self.factory
    }

    pub fn ports(&self) -> &[PortDefinition] {
        &self.ports
    }

    pub fn runtime_diagnostic(&self) -> Option<String> {
        (self.runtime_status == NativeNodeRuntimeStatus::DesignNeeded).then(|| {
            format!(
                "{} runtime/renderer is design-needed; evaluation produces No Output",
                self.label
            )
        })
    }

    pub fn create_detached_node(&self) -> Result<Node, String> {
        match self.factory {
            NativeNodeFactory::Generator(_) => Err(format!(
                "Native Generator '{}' requires its canvas-backed ProjectManager factory",
                self.catalog_id
            )),
            NativeNodeFactory::Value(value) => Ok(Node::new_value(self.label, value)),
            NativeNodeFactory::Merge => Ok(Node::new_merge(self.label)),
            NativeNodeFactory::TypedPlaceholder => Ok(Node::with_properties(
                self.label,
                NodeContent::NativeOperation(NativeOperationContent {
                    catalog_id: self.catalog_id.to_string(),
                }),
                PropertyMap::new(),
            )),
        }
    }
}

#[derive(Clone, Copy)]
struct PortSpec {
    key: &'static str,
    label: &'static str,
    data_type: PortDataType,
    multiplicity: PortMultiplicity,
}

impl PortSpec {
    const fn single(key: &'static str, label: &'static str, data_type: PortDataType) -> Self {
        Self {
            key,
            label,
            data_type,
            multiplicity: PortMultiplicity::Single,
        }
    }

    const fn variadic(key: &'static str, label: &'static str, data_type: PortDataType) -> Self {
        Self {
            key,
            label,
            data_type,
            multiplicity: PortMultiplicity::Variadic,
        }
    }

    fn input(self) -> PortDefinition {
        let mut definition = PortDefinition::input(self.key, self.label, self.data_type);
        definition.multiplicity = self.multiplicity;
        definition
    }

    fn output(self) -> PortDefinition {
        let mut definition = PortDefinition::output(
            self.key,
            self.label,
            self.data_type,
            PortSide::Right,
            PortExposure::Graph,
        );
        definition.multiplicity = self.multiplicity;
        definition
    }
}

#[derive(Clone, Copy)]
struct DescriptorSpec {
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    qa_id: &'static str,
    keywords: &'static [&'static str],
    runtime_status: NativeNodeRuntimeStatus,
    factory: NativeNodeFactory,
    inputs: &'static [PortSpec],
    outputs: &'static [PortSpec],
}

impl DescriptorSpec {
    fn build(self) -> NativeNodeCatalogDescriptor {
        let ports = self
            .inputs
            .iter()
            .copied()
            .map(PortSpec::input)
            .chain(self.outputs.iter().copied().map(PortSpec::output))
            .collect();
        NativeNodeCatalogDescriptor {
            catalog_id: self.catalog_id,
            label: self.label,
            category: self.category,
            qa_id: self.qa_id,
            keywords: self.keywords,
            runtime_status: self.runtime_status,
            factory: self.factory,
            ports,
        }
    }
}

const fn descriptor(
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    qa_id: &'static str,
    keywords: &'static [&'static str],
    factory: NativeNodeFactory,
    inputs: &'static [PortSpec],
    outputs: &'static [PortSpec],
) -> DescriptorSpec {
    DescriptorSpec {
        catalog_id,
        label,
        category,
        qa_id,
        keywords,
        runtime_status: NativeNodeRuntimeStatus::Implemented,
        factory,
        inputs,
        outputs,
    }
}

const fn placeholder(
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    inputs: &'static [PortSpec],
    outputs: &'static [PortSpec],
) -> DescriptorSpec {
    DescriptorSpec {
        catalog_id,
        label,
        category,
        qa_id: "node_editor.menu.create.catalog_placeholder",
        keywords: &["typed", "placeholder", "design-needed"],
        runtime_status: NativeNodeRuntimeStatus::DesignNeeded,
        factory: NativeNodeFactory::TypedPlaceholder,
        inputs,
        outputs,
    }
}

const TEXT_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("text", "Text", PortDataType::String),
    PortSpec::single("font_family", "Font", PortDataType::String),
    PortSpec::single("size", "Size", PortDataType::Number),
];
const SHAPE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    SHAPE_OUTPUT_PORT,
    "Shape",
    PortDataType::Shape,
)];
const SOLID_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("color", "Color", PortDataType::Color),
];
const SHAPE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("path", "Path", PortDataType::Path),
];
const SKSL_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("shader", "Shader", PortDataType::String),
];
const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];
const FMOD_INPUTS: &[PortSpec] = &[
    PortSpec::single(FMOD_X_INPUT_PORT, "X", PortDataType::Numeric),
    PortSpec::single(FMOD_DIVISOR_INPUT_PORT, "Divisor", PortDataType::Numeric),
];
const NUMERIC_INPUTS: &[PortSpec] = &[
    PortSpec::single(NUMERIC_A_INPUT_PORT, "A", PortDataType::Numeric),
    PortSpec::single(NUMERIC_B_INPUT_PORT, "B", PortDataType::Numeric),
];
const NUMERIC_OUTPUT: &[PortSpec] = &[PortSpec::single(
    NUMBER_RESULT_OUTPUT_PORT,
    "Result",
    PortDataType::Numeric,
)];
const MERGE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::variadic(MERGE_IMAGES_PORT, "Images", PortDataType::Image),
];

const PARTICLE: PortSpec = PortSpec::single("particles", "Particles", PortDataType::ParticleSystem);
const PARTICLE_OUTPUT: &[PortSpec] = &[PARTICLE];
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

const CAMERA_INPUTS: &[PortSpec] = &[
    PortSpec::single("position", "Position", PortDataType::Vec3),
    PortSpec::single("target", "Target", PortDataType::Vec3),
    PortSpec::single("up", "Up", PortDataType::Vec3),
    PortSpec::single("fov", "Fov", PortDataType::Number),
];
const CAMERA_OUTPUT: &[PortSpec] = &[PortSpec::single("camera", "Camera", PortDataType::Camera3D)];
const TRANSFORM_3D_INPUTS: &[PortSpec] = &[
    PortSpec::single("object", "Object", PortDataType::Object3D),
    PortSpec::single("translation", "Translation", PortDataType::Vec3),
    PortSpec::single("rotation", "Rotation", PortDataType::Vec3),
    PortSpec::single("scale", "Scale", PortDataType::Vec3),
];
const OBJECT_OUTPUT: &[PortSpec] = &[PortSpec::single("object", "Object", PortDataType::Object3D)];
const MESH_INSTANCE_INPUTS: &[PortSpec] = &[
    PortSpec::single("mesh_asset", "Mesh Asset", PortDataType::Asset),
    PortSpec::single("material", "Material", PortDataType::Material),
];
const RENDER_3D_INPUTS: &[PortSpec] = &[
    PortSpec::single("scene", "Scene", PortDataType::Object3DList),
    PortSpec::single("camera", "Camera", PortDataType::Camera3D),
    PortSpec::single("instances", "Instances", PortDataType::Instance3D),
];
const POINT_SOURCE_INPUTS: &[PortSpec] = &[
    PortSpec::single("geometry", "Geometry", PortDataType::Geometry3D),
    PortSpec::single("count", "Count", PortDataType::Integer),
    PortSpec::single("distribution", "Distribution", PortDataType::Enum),
];
const POINT_SOURCE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    "points",
    "Points",
    PortDataType::PointSource,
)];
const CLONER_INPUTS: &[PortSpec] = &[
    PortSpec::single("geometry", "Geometry", PortDataType::Geometry3D),
    PortSpec::single("object", "Object", PortDataType::Object3D),
    PortSpec::single("points", "Points", PortDataType::PointSource),
    PortSpec::single("count", "Count", PortDataType::Integer),
    PortSpec::single("effectors", "Effectors", PortDataType::EffectorStack),
    PortSpec::single("fields", "Fields", PortDataType::FieldStack),
    PortSpec::single(
        "motion_behavior",
        "Motion Behavior",
        PortDataType::MotionBehavior,
    ),
];
const INSTANCE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    "instances",
    "Instances",
    PortDataType::Instance3D,
)];
const TRANSFORM_EFFECTOR_INPUTS: &[PortSpec] = &[
    PortSpec::single("field", "Field", PortDataType::Field3D),
    PortSpec::single("translation", "Translation", PortDataType::Vec3),
    PortSpec::single("rotation", "Rotation", PortDataType::Vec3),
    PortSpec::single("scale", "Scale", PortDataType::Vec3),
    PortSpec::single(
        "motion_behavior",
        "Motion Behavior",
        PortDataType::MotionBehavior,
    ),
];
const EFFECTOR_OUTPUT: &[PortSpec] = &[PortSpec::single(
    "effector",
    "Effector",
    PortDataType::Effector3D,
)];
const EFFECTOR_STACK_INPUTS: &[PortSpec] = &[PortSpec::variadic(
    "effectors",
    "Effectors",
    PortDataType::Effector3D,
)];
const EFFECTOR_STACK_OUTPUT: &[PortSpec] = &[PortSpec::single(
    "effectors",
    "Effectors",
    PortDataType::EffectorStack,
)];
const FIELD_INPUTS: &[PortSpec] = &[
    PortSpec::single("field_type", "Field Type", PortDataType::Enum),
    PortSpec::single("position", "Position", PortDataType::Vec3),
    PortSpec::single("size", "Size", PortDataType::Vec3),
    PortSpec::single("falloff", "Falloff", PortDataType::Number),
];
const FIELD_OUTPUT: &[PortSpec] = &[PortSpec::single("field", "Field", PortDataType::Field3D)];
const FIELD_STACK_INPUTS: &[PortSpec] = &[PortSpec::variadic(
    "fields",
    "Fields",
    PortDataType::Field3D,
)];
const FIELD_STACK_OUTPUT: &[PortSpec] = &[PortSpec::single(
    "fields",
    "Fields",
    PortDataType::FieldStack,
)];
const MOTION_BEHAVIOR_INPUTS: &[PortSpec] = &[
    PortSpec::single("mode", "Mode", PortDataType::Enum),
    PortSpec::single("strength", "Strength", PortDataType::Number),
];
const MOTION_BEHAVIOR_OUTPUT: &[PortSpec] = &[PortSpec::single(
    "motion_behavior",
    "Motion Behavior",
    PortDataType::MotionBehavior,
)];

static DESCRIPTOR_SPECS: &[DescriptorSpec] = &[
    descriptor(
        "native.text",
        "Text",
        "Text",
        "node_editor.menu.create.text",
        &["title", "caption", "shape"],
        NativeNodeFactory::Generator(GeneratorContent::Text),
        TEXT_INPUTS,
        SHAPE_OUTPUT,
    ),
    descriptor(
        "native.solid-color",
        "Solid Color",
        "Generators",
        "node_editor.menu.create.solid",
        &["solid", "color", "image"],
        NativeNodeFactory::Generator(GeneratorContent::Solid),
        SOLID_INPUTS,
        IMAGE_OUTPUT,
    ),
    descriptor(
        "native.shape",
        "Shape",
        "Generators",
        "node_editor.menu.create.shape",
        &["shape", "rectangle", "path"],
        NativeNodeFactory::Generator(GeneratorContent::Shape),
        SHAPE_INPUTS,
        SHAPE_OUTPUT,
    ),
    descriptor(
        "native.sksl-shader",
        "SkSL Shader",
        "Generators",
        "node_editor.menu.create.sksl",
        &["sksl", "shader", "procedural", "image"],
        NativeNodeFactory::Generator(GeneratorContent::SkSL),
        SKSL_INPUTS,
        IMAGE_OUTPUT,
    ),
    descriptor(
        "native.math.fmod",
        "Fmod",
        "Math",
        "node_editor.menu.create.value:fmod",
        &["modulo", "remainder", "loop", "number"],
        NativeNodeFactory::Value(ValueContent::Fmod),
        FMOD_INPUTS,
        NUMERIC_OUTPUT,
    ),
    descriptor(
        "native.math.add",
        "Add",
        "Math",
        "node_editor.menu.create.value:add",
        &["plus", "sum", "number"],
        NativeNodeFactory::Value(ValueContent::Add),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    descriptor(
        "native.math.subtract",
        "Subtract",
        "Math",
        "node_editor.menu.create.value:subtract",
        &["minus", "difference", "number"],
        NativeNodeFactory::Value(ValueContent::Subtract),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    descriptor(
        "native.math.multiply",
        "Multiply",
        "Math",
        "node_editor.menu.create.value:multiply",
        &["times", "product", "number"],
        NativeNodeFactory::Value(ValueContent::Multiply),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    descriptor(
        "native.math.divide",
        "Divide",
        "Math",
        "node_editor.menu.create.value:divide",
        &["quotient", "ratio", "number"],
        NativeNodeFactory::Value(ValueContent::Divide),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    descriptor(
        "native.merge",
        "Merge",
        "Compositing",
        "node_editor.menu.create.merge",
        &["composite", "blend", "layers"],
        NativeNodeFactory::Merge,
        MERGE_INPUTS,
        IMAGE_OUTPUT,
    ),
    placeholder(
        "native.particle.emitter",
        "Particle Emitter",
        "Particles",
        PARTICLE_EMITTER_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.spawn-burst",
        "Spawn Burst",
        "Particles",
        SPAWN_BURST_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.shape-location",
        "Shape Location",
        "Particles",
        SHAPE_LOCATION_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.initialize",
        "Initialize Particle",
        "Particles",
        INITIALIZE_PARTICLE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.set-attribute",
        "Set Attribute",
        "Particles",
        SET_ATTRIBUTE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.gravity-force",
        "Gravity Force",
        "Particles",
        GRAVITY_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.drag-force",
        "Drag Force",
        "Particles",
        DRAG_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.point-force",
        "Point Force",
        "Particles",
        POINT_FORCE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.vortex-force",
        "Vortex Force",
        "Particles",
        VORTEX_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.vector-field-force",
        "Vector Field Force",
        "Particles",
        VECTOR_FIELD_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.turbulence",
        "Turbulence",
        "Particles",
        TURBULENCE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.color-over-life",
        "Color Over Life",
        "Particles",
        COLOR_OVER_LIFE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.size-over-life",
        "Size Over Life",
        "Particles",
        SIZE_OVER_LIFE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.collision-plane",
        "Collision Plane",
        "Particles",
        COLLISION_PLANE_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.collision-depth",
        "Collision Depth",
        "Particles",
        COLLISION_DEPTH_INPUTS,
        PARTICLE_OUTPUT,
    ),
    placeholder(
        "native.particle.sprite-renderer",
        "Sprite Renderer",
        "Particles",
        SPRITE_RENDERER_INPUTS,
        IMAGE_OUTPUT,
    ),
    placeholder(
        "native.particle.mesh-renderer",
        "Mesh Renderer",
        "Particles",
        MESH_RENDERER_INPUTS,
        IMAGE_OUTPUT,
    ),
    placeholder(
        "native.particle.ribbon-renderer",
        "Ribbon Renderer",
        "Particles",
        RIBBON_RENDERER_INPUTS,
        IMAGE_OUTPUT,
    ),
    placeholder(
        "native.3d.camera",
        "Camera 3D",
        "3D",
        CAMERA_INPUTS,
        CAMERA_OUTPUT,
    ),
    placeholder(
        "native.3d.transform",
        "Transform 3D",
        "3D",
        TRANSFORM_3D_INPUTS,
        OBJECT_OUTPUT,
    ),
    placeholder(
        "native.3d.mesh-instance",
        "Mesh Instance",
        "3D",
        MESH_INSTANCE_INPUTS,
        OBJECT_OUTPUT,
    ),
    placeholder(
        "native.3d.render",
        "Render 3D",
        "3D",
        RENDER_3D_INPUTS,
        IMAGE_OUTPUT,
    ),
    placeholder(
        "native.3d.point-source",
        "Point Source 3D",
        "3D",
        POINT_SOURCE_INPUTS,
        POINT_SOURCE_OUTPUT,
    ),
    placeholder(
        "native.3d.cloner",
        "Cloner 3D",
        "3D",
        CLONER_INPUTS,
        INSTANCE_OUTPUT,
    ),
    placeholder(
        "native.3d.transform-effector",
        "Transform Effector 3D",
        "3D",
        TRANSFORM_EFFECTOR_INPUTS,
        EFFECTOR_OUTPUT,
    ),
    placeholder(
        "native.3d.effector-stack",
        "Effector Stack 3D",
        "3D",
        EFFECTOR_STACK_INPUTS,
        EFFECTOR_STACK_OUTPUT,
    ),
    placeholder(
        "native.3d.field",
        "Field 3D",
        "3D",
        FIELD_INPUTS,
        FIELD_OUTPUT,
    ),
    placeholder(
        "native.3d.field-stack",
        "Field Stack 3D",
        "3D",
        FIELD_STACK_INPUTS,
        FIELD_STACK_OUTPUT,
    ),
    placeholder(
        "native.motion.behavior",
        "Motion Behavior",
        "3D",
        MOTION_BEHAVIOR_INPUTS,
        MOTION_BEHAVIOR_OUTPUT,
    ),
];

static NATIVE_NODE_CATALOG: LazyLock<Vec<NativeNodeCatalogDescriptor>> = LazyLock::new(|| {
    DESCRIPTOR_SPECS
        .iter()
        .copied()
        .map(DescriptorSpec::build)
        .collect()
});

pub fn native_node_catalog() -> &'static [NativeNodeCatalogDescriptor] {
    NATIVE_NODE_CATALOG.as_slice()
}

pub fn native_node_descriptor(catalog_id: &str) -> Option<&'static NativeNodeCatalogDescriptor> {
    native_node_catalog()
        .iter()
        .find(|descriptor| descriptor.catalog_id == catalog_id)
}

pub fn native_node_descriptor_for_node(
    node: &Node,
) -> Option<&'static NativeNodeCatalogDescriptor> {
    match node.content() {
        NodeContent::Generator(generator) => {
            let catalog_id = match generator {
                GeneratorContent::Text => "native.text",
                GeneratorContent::Solid => "native.solid-color",
                GeneratorContent::Shape => "native.shape",
                GeneratorContent::SkSL => "native.sksl-shader",
            };
            native_node_descriptor(catalog_id)
        }
        NodeContent::Value(value) => native_node_catalog().iter().find(|descriptor| {
            matches!(descriptor.factory, NativeNodeFactory::Value(candidate) if candidate == *value)
        }),
        NodeContent::Merge => native_node_descriptor("native.merge"),
        NodeContent::NativeOperation(operation) => native_node_descriptor(&operation.catalog_id),
        NodeContent::Media(_)
        | NodeContent::CompositionInstance(_)
        | NodeContent::PluginOperation(_) => None,
    }
}
