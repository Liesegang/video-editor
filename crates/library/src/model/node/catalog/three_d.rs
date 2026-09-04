//! Typed 3D and motion-graphics node contracts whose runtime is design-needed.

use super::descriptor::{DescriptorSpec, PortSpec};
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};

const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];
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

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::placeholder(
        "native.3d.camera",
        "Camera 3D",
        "3D",
        CAMERA_INPUTS,
        CAMERA_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.transform",
        "Transform 3D",
        "3D",
        TRANSFORM_3D_INPUTS,
        OBJECT_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.mesh-instance",
        "Mesh Instance",
        "3D",
        MESH_INSTANCE_INPUTS,
        OBJECT_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.render",
        "Render 3D",
        "3D",
        RENDER_3D_INPUTS,
        IMAGE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.point-source",
        "Point Source 3D",
        "3D",
        POINT_SOURCE_INPUTS,
        POINT_SOURCE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.cloner",
        "Cloner 3D",
        "3D",
        CLONER_INPUTS,
        INSTANCE_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.transform-effector",
        "Transform Effector 3D",
        "3D",
        TRANSFORM_EFFECTOR_INPUTS,
        EFFECTOR_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.effector-stack",
        "Effector Stack 3D",
        "3D",
        EFFECTOR_STACK_INPUTS,
        EFFECTOR_STACK_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.field",
        "Field 3D",
        "3D",
        FIELD_INPUTS,
        FIELD_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.3d.field-stack",
        "Field Stack 3D",
        "3D",
        FIELD_STACK_INPUTS,
        FIELD_STACK_OUTPUT,
    ),
    DescriptorSpec::placeholder(
        "native.motion.behavior",
        "Motion Behavior",
        "3D",
        MOTION_BEHAVIOR_INPUTS,
        MOTION_BEHAVIOR_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
