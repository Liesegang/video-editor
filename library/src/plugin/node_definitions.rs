//! Hardcoded node type definitions for the data-flow graph.
//!
//! Reference: node_list.yml (documentation only, not loaded at runtime).

use crate::model::project::connection::{PinDataType, PinDefinition};
use crate::plugin::PluginManager;
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};

/// Register all built-in node type definitions.
pub(crate) fn register_all_node_types(manager: &PluginManager) {
    for def in all_node_definitions() {
        manager.register_node_type(def);
    }
}

// ---------------------------------------------------------------------------
// Pin helpers
// ---------------------------------------------------------------------------

fn inp(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::input(name, display, dt)
}

fn out(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::output(name, display, dt)
}

fn node(type_id: &str, name: &str, cat: NodeCategory) -> NodeTypeDefinition {
    NodeTypeDefinition::new(type_id, name, cat)
}

// ---------------------------------------------------------------------------
// Pattern helpers — eliminate repeated boilerplate
// ---------------------------------------------------------------------------

/// Blend node: foreground + background + opacity → image.
fn blend_node(type_id: &str, name: &str) -> NodeTypeDefinition {
    use PinDataType::*;
    node(type_id, name, NodeCategory::Compositing)
        .with_inputs(vec![
            inp("foreground", "Foreground", Image),
            inp("background", "Background", Image),
            inp("opacity", "Opacity", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)])
}

/// Particle modifier: particles in + extra params → particles out.
fn particle_modifier(
    type_id: &str,
    name: &str,
    extra_inputs: Vec<PinDefinition>,
) -> NodeTypeDefinition {
    use PinDataType::*;
    let mut inputs = vec![inp("particles", "Particles", ParticleSystem)];
    inputs.extend(extra_inputs);
    node(type_id, name, NodeCategory::Particles)
        .with_inputs(inputs)
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)])
}

/// Image filter: image in + extra params → image out.
fn image_filter(
    type_id: &str,
    name: &str,
    cat: NodeCategory,
    extra_inputs: Vec<PinDefinition>,
) -> NodeTypeDefinition {
    use PinDataType::*;
    let mut inputs = vec![inp("image", "Image", Image)];
    inputs.extend(extra_inputs);
    node(type_id, name, cat)
        .with_inputs(inputs)
        .with_outputs(vec![out("image", "Image", Image)])
}

// ---------------------------------------------------------------------------
// Category functions
// ---------------------------------------------------------------------------

fn all_node_definitions() -> Vec<NodeTypeDefinition> {
    [
        scripting_nodes(),
        data_nodes(),
        text_nodes(),
        math_nodes(),
        logic_nodes(),
        compositing_nodes(),
        color_nodes(),
        generator_nodes(),
        filter_nodes(),
        path_nodes(),
        time_nodes(),
        image_nodes(),
        particle_nodes(),
        threed_nodes(),
    ]
    .concat()
}

fn scripting_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    vec![
        node(
            "scripting.expression",
            "Expression",
            NodeCategory::Scripting,
        )
        .with_description("Execute custom Python scripts")
        .with_inputs(vec![
            inp("code", "Code", String),
            inp("inputs", "Inputs", List),
        ])
        .with_outputs(vec![out("result", "Result", Any)]),
    ]
}

fn data_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Data;
    vec![
        node("data.scalar", "Scalar", nc)
            .with_description("Single numeric value")
            .with_outputs(vec![out("value", "Value", Scalar)]),
        node("data.vector", "Vector", nc)
            .with_description("Generic Vector")
            .with_outputs(vec![out("value", "Value", Vector)]),
        node("data.blank", "Blank", nc)
            .with_description("Empty generic data/image")
            .with_outputs(vec![out("value", "Value", Any)]),
        node("data.vector2", "Vector2", nc)
            .with_description("2D Vector")
            .with_outputs(vec![out("value", "Value", Vec2)]),
        node("data.vector3", "Vector3", nc)
            .with_description("3D Vector")
            .with_outputs(vec![out("value", "Value", Vec3)]),
        node("data.color", "Color", nc)
            .with_description("RGBA Color")
            .with_outputs(vec![out("value", "Value", Color)]),
        node("data.string", "String", nc)
            .with_description("Text string")
            .with_outputs(vec![out("value", "Value", String)]),
        node("data.image", "Image", nc)
            .with_description("Generic Image buffer")
            .with_outputs(vec![out("value", "Value", Image)]),
        node("data.video", "Video", nc)
            .with_description("Video resource")
            .with_inputs(vec![inp("path", "Path", String)])
            .with_outputs(vec![out("output", "Output", Video)]),
        node("data.rgb_image", "RGB Image", nc)
            .with_description("Image in RGB color space")
            .with_outputs(vec![out("output", "Output", Image)]),
        node("data.yuv_image", "YUV Image", nc)
            .with_description("Image in YUV color space")
            .with_outputs(vec![out("output", "Output", Image)]),
        node("data.gradient", "Gradient", nc)
            .with_description("Color gradient (Color Ramp)")
            .with_outputs(vec![out("output", "Output", Gradient)]),
        node("data.curve", "Curve", nc)
            .with_description("1D Value Curve (Profile/Timeline)")
            .with_outputs(vec![out("output", "Output", Curve)]),
        node("data.asset", "Asset", nc)
            .with_inputs(vec![inp("asset_id", "Asset ID", String)])
            .with_outputs(vec![out("output", "Output", Any)]),
    ]
}

fn text_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Text;
    vec![
        node("text.text", "Text", nc)
            .with_inputs(vec![
                inp("string", "String", String),
                inp("font", "Font", Font),
                inp("size", "Size", Scalar),
                inp("italic", "Italic", Boolean),
                inp("bold", "Bold", Scalar),
                inp("split_mode", "Split Mode", Enum),
                inp("sort_order", "Sort Order", Enum),
            ])
            .with_outputs(vec![
                out("path", "Path", Path),
                out("line_index", "Line Index", List),
                out("char_index", "Char Index", List),
                out("stroke_index", "Stroke Index", List),
            ]),
        node("text.join_strings", "Join Strings", nc)
            .with_inputs(vec![
                inp("strings", "Strings", List),
                inp("separator", "Separator", String),
            ])
            .with_outputs(vec![out("result", "Result", String)]),
        node("text.replace_string", "Replace String", nc)
            .with_inputs(vec![
                inp("source", "Source", String),
                inp("from", "From", String),
                inp("to", "To", String),
            ])
            .with_outputs(vec![out("result", "Result", String)]),
    ]
}

fn math_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Math;
    vec![
        node("math.add", "Add", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.subtract", "Subtract", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.multiply", "Multiply", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.divide", "Divide", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.power", "Power", nc)
            .with_inputs(vec![
                inp("base", "Base", Any),
                inp("exponent", "Exponent", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.clamp", "Clamp", nc)
            .with_inputs(vec![
                inp("value", "Value", Any),
                inp("min", "Min", Any),
                inp("max", "Max", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.remap", "Remap", nc)
            .with_description("Remap value from input range to output range")
            .with_inputs(vec![
                inp("value", "Value", Any),
                inp("in_min", "In Min", Any),
                inp("in_max", "In Max", Any),
                inp("out_min", "Out Min", Any),
                inp("out_max", "Out Max", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        // Vector Math
        node("math.dot_product", "Dot Product", nc)
            .with_inputs(vec![inp("a", "A", Vector), inp("b", "B", Vector)])
            .with_outputs(vec![out("result", "Result", Scalar)]),
        node("math.cross_product", "Cross Product", nc)
            .with_inputs(vec![inp("a", "A", Vector), inp("b", "B", Vector)])
            .with_outputs(vec![out("result", "Result", Vector)]),
        node("math.normalize", "Normalize", nc)
            .with_inputs(vec![inp("vector", "Vector", Vector)])
            .with_outputs(vec![out("result", "Result", Vector)]),
    ]
}

fn logic_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Logic;
    vec![
        node("logic.switch", "Switch", nc)
            .with_inputs(vec![
                inp("condition", "Condition", Boolean),
                inp("true_val", "True", Any),
                inp("false_val", "False", Any),
            ])
            .with_outputs(vec![out("output", "Output", Any)]),
        node("logic.make_list", "Make List", nc)
            .with_description("Create a list from inputs")
            .with_inputs(vec![inp("item", "Item", Any)])
            .with_outputs(vec![out("list", "List", List)]),
        node("logic.get_list_item", "Get List Item", nc)
            .with_inputs(vec![
                inp("list", "List", List),
                inp("index", "Index", Integer),
            ])
            .with_outputs(vec![out("item", "Item", Any)]),
        node("logic.list_length", "List Length", nc)
            .with_inputs(vec![inp("list", "List", List)])
            .with_outputs(vec![out("length", "Length", Integer)]),
    ]
}

fn compositing_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Compositing;
    vec![
        node("compositing.transform", "Transform", nc)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("position", "Position", Vec2),
                inp("rotation", "Rotation", Scalar),
                inp("scale", "Scale", Vec2),
                inp("anchor", "Anchor", Vec2),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("compositing.composite", "Composite", nc)
            .with_description("Blend N images with individual blend modes")
            .with_inputs(vec![
                inp("layers", "Layers", List),
                inp("blend_modes", "Blend Modes", List),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // Blend nodes (share identical pin layout)
        blend_node("compositing.normal_blend", "Normal Blend"),
        blend_node("compositing.multiply_blend", "Multiply Blend"),
        blend_node("compositing.screen_blend", "Screen Blend"),
        blend_node("compositing.overlay_blend", "Overlay Blend"),
        node("compositing.mask", "Mask", nc)
            .with_inputs(vec![
                inp("source", "Source", Image),
                inp("mask", "Mask", Image),
                inp("mode", "Mode", Enum),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
    ]
}

fn color_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Color;
    vec![
        image_filter(
            "color.color_correction",
            "Color Correction",
            nc,
            vec![
                inp("lift", "Lift", Color),
                inp("gamma", "Gamma", Color),
                inp("gain", "Gain", Color),
                inp("offset", "Offset", Color),
            ],
        )
        .with_description("Lift, Gamma, Gain 3-way color correction"),
        image_filter(
            "color.brightness_contrast",
            "Brightness Contrast",
            nc,
            vec![
                inp("brightness", "Brightness", Scalar),
                inp("contrast", "Contrast", Scalar),
            ],
        ),
        image_filter(
            "color.hue_saturation",
            "Hue Saturation",
            nc,
            vec![
                inp("hue", "Hue", Scalar),
                inp("saturation", "Saturation", Scalar),
                inp("lightness", "Lightness", Scalar),
            ],
        ),
        image_filter(
            "color.levels",
            "Levels",
            nc,
            vec![
                inp("black_in", "Black In", Scalar),
                inp("white_in", "White In", Scalar),
                inp("gamma", "Gamma", Scalar),
                inp("black_out", "Black Out", Scalar),
                inp("white_out", "White Out", Scalar),
            ],
        ),
        image_filter("color.invert", "Invert", nc, vec![]),
        image_filter(
            "color.hue_shift",
            "Hue Shift",
            nc,
            vec![inp("shift_degrees", "Shift Degrees", Scalar)],
        ),
        image_filter(
            "color.color_map",
            "Color Map",
            nc,
            vec![
                inp("gradient", "Gradient", Gradient),
                inp("gradient_map", "Gradient Map", Image),
            ],
        )
        .with_description("Map luminance to a color gradient"),
    ]
}

fn generator_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Generator;
    vec![
        node("generators.solid_color", "Solid Color", nc)
            .with_inputs(vec![inp("color", "Color", Color)])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("generators.linear_gradient", "Linear Gradient", nc)
            .with_inputs(vec![
                inp("start_point", "Start Point", Vec2),
                inp("end_point", "End Point", Vec2),
                inp("gradient", "Gradient", Gradient),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("generators.radial_gradient", "Radial Gradient", nc)
            .with_inputs(vec![
                inp("center", "Center", Vec2),
                inp("radius", "Radius", Scalar),
                inp("gradient", "Gradient", Gradient),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("generators.construct_gradient", "Construct Gradient", nc)
            .with_description("Create a gradient from color stops")
            .with_inputs(vec![
                inp("colors", "Colors", List),
                inp("positions", "Positions", List),
            ])
            .with_outputs(vec![out("gradient", "Gradient", Gradient)]),
        node("generators.sample_gradient", "Sample Gradient", nc)
            .with_inputs(vec![
                inp("gradient", "Gradient", Gradient),
                inp("time", "Time", Scalar),
            ])
            .with_outputs(vec![out("color", "Color", Color)]),
        node("generators.evaluate_curve", "Evaluate Curve", nc)
            .with_inputs(vec![
                inp("curve", "Curve", Curve),
                inp("time", "Time", Scalar),
            ])
            .with_outputs(vec![out("value", "Value", Scalar)]),
        node("generators.noise", "Noise", nc)
            .with_inputs(vec![
                inp("scale", "Scale", Scalar),
                inp("seed", "Seed", Integer),
                inp("evolution", "Evolution", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
    ]
}

fn filter_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    vec![
        image_filter(
            "filters.blur",
            "Blur",
            NodeCategory::Filters,
            vec![
                inp("radius_x", "Radius X", Scalar),
                inp("radius_y", "Radius Y", Scalar),
            ],
        )
        .with_description("Gaussian blur"),
        image_filter(
            "filters.glow",
            "Glow",
            NodeCategory::Filters,
            vec![
                inp("threshold", "Threshold", Scalar),
                inp("radius", "Radius", Scalar),
                inp("intensity", "Intensity", Scalar),
            ],
        ),
        image_filter(
            "filters.drop_shadow",
            "Drop Shadow",
            NodeCategory::Filters,
            vec![
                inp("color", "Color", Color),
                inp("distance", "Distance", Scalar),
                inp("angle", "Angle", Scalar),
                inp("softness", "Softness", Scalar),
            ],
        ),
        // Distortion (grouped with filters for convenience)
        image_filter(
            "distortion.displacement_map",
            "Displacement Map",
            NodeCategory::Distortion,
            vec![
                inp("map", "Map", Image),
                inp("scale_x", "Scale X", Scalar),
                inp("scale_y", "Scale Y", Scalar),
            ],
        ),
    ]
}

fn path_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Path;
    vec![
        node("path.offset_path", "Offset Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("offset", "Offset", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.fill_path", "Fill Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("color", "Color", Color),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("path.stroke_path", "Stroke Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("color", "Color", Color),
                inp("width", "Width", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("path.union_path", "Union Path", nc)
            .with_inputs(vec![inp("paths", "Paths", List)])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.trim_path", "Trim Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("start", "Start", Scalar),
                inp("end", "End", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.corner_path", "Corner Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("radius", "Radius", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.discrete_path", "Discrete Path", nc)
            .with_description("Jitter the path (DiscretePathEffect)")
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("segment_length", "Segment Length", Scalar),
                inp("deviation", "Deviation", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.simplify_path", "Simplify Path", nc)
            .with_description("Reduce points in path")
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("tolerance", "Tolerance", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
    ]
}

fn time_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    vec![image_filter(
        "time.time_shift",
        "Time Shift",
        NodeCategory::Time,
        vec![inp("time_offset", "Time Offset", Scalar)],
    )]
}

fn image_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Image;
    vec![
        node("image.channel_split", "Channel Split", nc)
            .with_inputs(vec![inp("image", "Image", Image)])
            .with_outputs(vec![
                out("r", "R", Image),
                out("g", "G", Image),
                out("b", "B", Image),
                out("a", "A", Image),
            ]),
        node("image.channel_combine", "Channel Combine", nc)
            .with_inputs(vec![
                inp("r", "R", Image),
                inp("g", "G", Image),
                inp("b", "B", Image),
                inp("a", "A", Image),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("image.rgb_to_yuv", "RGB to YUV", nc)
            .with_inputs(vec![inp("rgb_image", "RGB Image", Image)])
            .with_outputs(vec![out("yuv_image", "YUV Image", Image)]),
        node("image.yuv_to_rgb", "YUV to RGB", nc)
            .with_inputs(vec![inp("yuv_image", "YUV Image", Image)])
            .with_outputs(vec![out("rgb_image", "RGB Image", Image)]),
    ]
}

fn particle_nodes() -> Vec<NodeTypeDefinition> {
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

fn threed_nodes() -> Vec<NodeTypeDefinition> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_node_definitions_count() {
        let defs = all_node_definitions();
        assert_eq!(defs.len(), 92, "Expected 92 node definitions");
    }

    #[test]
    fn test_no_duplicate_type_ids() {
        let defs = all_node_definitions();
        let mut seen = std::collections::HashSet::new();
        for def in &defs {
            assert!(
                seen.insert(&def.type_id),
                "Duplicate type_id: {}",
                def.type_id
            );
        }
    }

    #[test]
    fn test_all_categories_covered() {
        let defs = all_node_definitions();
        let categories: std::collections::HashSet<_> = defs.iter().map(|d| d.category).collect();

        assert!(categories.contains(&NodeCategory::Scripting));
        assert!(categories.contains(&NodeCategory::Data));
        assert!(categories.contains(&NodeCategory::Text));
        assert!(categories.contains(&NodeCategory::Math));
        assert!(categories.contains(&NodeCategory::Logic));
        assert!(categories.contains(&NodeCategory::Compositing));
        assert!(categories.contains(&NodeCategory::Color));
        assert!(categories.contains(&NodeCategory::Generator));
        assert!(categories.contains(&NodeCategory::Filters));
        assert!(categories.contains(&NodeCategory::Distortion));
        assert!(categories.contains(&NodeCategory::Path));
        assert!(categories.contains(&NodeCategory::Time));
        assert!(categories.contains(&NodeCategory::Image));
        assert!(categories.contains(&NodeCategory::Particles));
        assert!(categories.contains(&NodeCategory::ThreeD));
    }
}
