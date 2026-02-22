use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn threed_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::ThreeD;
    vec![
        node("3d.camera_3d", "Camera 3D", nc)
            .with_inputs(vec![
                inp("position", "Position", Vec3),
                inp("target", "Target", Vec3),
                inp("up", "Up", Vec3),
                inp("fov", "FOV", Scalar),
            ])
            .with_outputs(vec![out("camera", "Camera", Camera3D)]),
        node("3d.transform_3d", "Transform 3D", nc)
            .with_inputs(vec![
                inp("object", "Object", Object3D),
                inp("translation", "Translation", Vec3),
                inp("rotation", "Rotation", Vec3),
                inp("scale", "Scale", Vec3),
            ])
            .with_outputs(vec![out("object", "Object", Object3D)]),
        node("3d.mesh_instance", "Mesh Instance", nc)
            .with_inputs(vec![
                inp("mesh_asset", "Mesh Asset", Any),
                inp("material", "Material", Material),
            ])
            .with_outputs(vec![out("object", "Object", Object3D)]),
        node("3d.render_3d", "Render 3D", nc)
            .with_inputs(vec![
                inp("scene", "Scene", List),
                inp("camera", "Camera", Camera3D),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
    ]
}
