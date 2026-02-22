use super::{inp, node, out, particle_modifier};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn particle_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Particles;
    vec![
        // Emitter (unique structure — not a simple modifier)
        node("particles.particle_emitter", "Particle Emitter", nc)
            .with_description("Base emitter context (GPU Compute)")
            .with_inputs(vec![
                inp("capacity", "Capacity", Integer),
                inp("simulation_space", "Simulation Space", Enum),
                inp("rate", "Rate", Scalar),
                inp("lifetime", "Lifetime", Scalar),
                inp("loop", "Loop", Boolean),
                inp("duration", "Duration", Scalar),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        // Modifiers (all share particles-in → particles-out)
        particle_modifier(
            "particles.spawn_burst",
            "Spawn Burst",
            vec![inp("count", "Count", Integer), inp("time", "Time", Scalar)],
        ),
        particle_modifier(
            "particles.shape_location",
            "Shape Location",
            vec![
                inp("shape", "Shape", Enum),
                inp("radius", "Radius", Scalar),
                inp("size", "Size", Vec3),
                inp("surface_only", "Surface Only", Boolean),
            ],
        )
        .with_description("Initialize particle positions on a shape"),
        particle_modifier(
            "particles.initialize_particle",
            "Initialize Particle",
            vec![
                inp("velocity_min", "Velocity Min", Vec3),
                inp("velocity_max", "Velocity Max", Vec3),
                inp("color_min", "Color Min", Color),
                inp("color_max", "Color Max", Color),
                inp("size_min", "Size Min", Scalar),
                inp("size_max", "Size Max", Scalar),
            ],
        ),
        particle_modifier(
            "particles.set_attribute",
            "Set Attribute",
            vec![
                inp("attribute_name", "Attribute Name", String),
                inp("value", "Value", Any),
            ],
        )
        .with_description("Set custom particle attribute (GPU Buffer Write)"),
        particle_modifier(
            "particles.gravity_force",
            "Gravity Force",
            vec![inp("force", "Force", Vec3)],
        ),
        particle_modifier(
            "particles.drag_force",
            "Drag Force",
            vec![inp("coefficient", "Coefficient", Scalar)],
        ),
        particle_modifier(
            "particles.point_force",
            "Point Force",
            vec![
                inp("target", "Target", Vec3),
                inp("strength", "Strength", Scalar),
                inp("radius", "Radius", Scalar),
                inp("falloff", "Falloff", Scalar),
            ],
        )
        .with_description("Attract or repel particles"),
        particle_modifier(
            "particles.vortex_force",
            "Vortex Force",
            vec![
                inp("axis", "Axis", Vec3),
                inp("strength", "Strength", Scalar),
            ],
        ),
        particle_modifier(
            "particles.vector_field_force",
            "Vector Field Force",
            vec![
                inp("vector_field", "Vector Field", Any),
                inp("intensity", "Intensity", Scalar),
                inp("tiling", "Tiling", Vec3),
            ],
        )
        .with_description("3D Vector Field interaction (GPU Texture 3D)"),
        particle_modifier(
            "particles.turbulence",
            "Turbulence",
            vec![
                inp("frequency", "Frequency", Scalar),
                inp("strength", "Strength", Scalar),
                inp("octave", "Octave", Integer),
            ],
        ),
        particle_modifier(
            "particles.color_over_life",
            "Color Over Life",
            vec![inp("gradient", "Gradient", Gradient)],
        ),
        particle_modifier(
            "particles.size_over_life",
            "Size Over Life",
            vec![inp("curve", "Curve", Curve)],
        ),
        particle_modifier(
            "particles.collision_plane",
            "Collision Plane",
            vec![
                inp("plane_point", "Plane Point", Vec3),
                inp("plane_normal", "Plane Normal", Vec3),
                inp("bounce", "Bounce", Scalar),
                inp("friction", "Friction", Scalar),
            ],
        ),
        particle_modifier(
            "particles.collision_depth",
            "Collision Depth",
            vec![
                inp("depth_buffer", "Depth Buffer", Image),
                inp("thickness", "Thickness", Scalar),
            ],
        )
        .with_description("Screen-space depth collision (GPU)"),
        // Renderers (particles → image)
        node("particles.sprite_renderer", "Sprite Renderer", nc)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("texture", "Texture", Image),
                inp("color", "Color", Color),
                inp("blend_mode", "Blend Mode", Enum),
                inp("alignment", "Alignment", Enum),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("particles.mesh_renderer", "Mesh Renderer", nc)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("mesh", "Mesh", Any),
                inp("material", "Material", Material),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("particles.ribbon_renderer", "Ribbon Renderer", nc)
            .with_description("Connect particles with a trail")
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("texture", "Texture", Image),
                inp("width", "Width", Scalar),
                inp("max_trail_length", "Max Trail Length", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
    ]
}
