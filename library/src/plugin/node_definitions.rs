//! Hardcoded node type definitions for the data-flow graph.
//!
//! Reference: node_list.yml (documentation only, not loaded at runtime).

use crate::model::project::connection::{PinDataType, PinDefinition};
use crate::plugin::PluginManager;
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};

/// Register all built-in node type definitions.
pub fn register_all_node_types(manager: &PluginManager) {
    for def in all_node_definitions() {
        manager.register_node_type(def);
    }
}

/// Helper: create input pin.
fn inp(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::input(name, display, dt)
}

/// Helper: create output pin.
fn out(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::output(name, display, dt)
}

/// Helper: shorthand for building a node.
fn node(type_id: &str, name: &str, cat: NodeCategory) -> NodeTypeDefinition {
    NodeTypeDefinition::new(type_id, name, cat)
}

fn all_node_definitions() -> Vec<NodeTypeDefinition> {
    use NodeCategory as NC;
    use PinDataType::*;

    vec![
        // ==================== Scripting ====================
        node("scripting.expression", "Expression", NC::Scripting)
            .with_description("Execute custom Python scripts")
            .with_inputs(vec![
                inp("code", "Code", String),
                inp("inputs", "Inputs", List),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        // ==================== Data ====================
        node("data.scalar", "Scalar", NC::Data)
            .with_description("Single numeric value")
            .with_outputs(vec![out("value", "Value", Scalar)]),
        node("data.vector", "Vector", NC::Data)
            .with_description("Generic Vector")
            .with_outputs(vec![out("value", "Value", Vector)]),
        node("data.blank", "Blank", NC::Data)
            .with_description("Empty generic data/image")
            .with_outputs(vec![out("value", "Value", Any)]),
        node("data.vector2", "Vector2", NC::Data)
            .with_description("2D Vector")
            .with_outputs(vec![out("value", "Value", Vec2)]),
        node("data.vector3", "Vector3", NC::Data)
            .with_description("3D Vector")
            .with_outputs(vec![out("value", "Value", Vec3)]),
        node("data.color", "Color", NC::Data)
            .with_description("RGBA Color")
            .with_outputs(vec![out("value", "Value", Color)]),
        node("data.string", "String", NC::Data)
            .with_description("Text string")
            .with_outputs(vec![out("value", "Value", String)]),
        node("data.image", "Image", NC::Data)
            .with_description("Generic Image buffer")
            .with_outputs(vec![out("value", "Value", Image)]),
        node("data.video", "Video", NC::Data)
            .with_description("Video resource")
            .with_inputs(vec![inp("path", "Path", String)])
            .with_outputs(vec![out("output", "Output", Video)]),
        node("data.rgb_image", "RGB Image", NC::Data)
            .with_description("Image in RGB color space")
            .with_outputs(vec![out("output", "Output", Image)]),
        node("data.yuv_image", "YUV Image", NC::Data)
            .with_description("Image in YUV color space")
            .with_outputs(vec![out("output", "Output", Image)]),
        node("data.gradient", "Gradient", NC::Data)
            .with_description("Color gradient (Color Ramp)")
            .with_outputs(vec![out("output", "Output", Gradient)]),
        node("data.curve", "Curve", NC::Data)
            .with_description("1D Value Curve (Profile/Timeline)")
            .with_outputs(vec![out("output", "Output", Curve)]),
        node("data.asset", "Asset", NC::Data)
            .with_inputs(vec![inp("asset_id", "Asset ID", String)])
            .with_outputs(vec![out("output", "Output", Any)]),
        // ==================== Text ====================
        node("text.text", "Text", NC::Text)
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
        node("text.join_strings", "Join Strings", NC::Text)
            .with_inputs(vec![
                inp("strings", "Strings", List),
                inp("separator", "Separator", String),
            ])
            .with_outputs(vec![out("result", "Result", String)]),
        node("text.replace_string", "Replace String", NC::Text)
            .with_inputs(vec![
                inp("source", "Source", String),
                inp("from", "From", String),
                inp("to", "To", String),
            ])
            .with_outputs(vec![out("result", "Result", String)]),
        // ==================== Math ====================
        node("math.add", "Add", NC::Math)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.subtract", "Subtract", NC::Math)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.multiply", "Multiply", NC::Math)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.divide", "Divide", NC::Math)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.power", "Power", NC::Math)
            .with_inputs(vec![
                inp("base", "Base", Any),
                inp("exponent", "Exponent", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.clamp", "Clamp", NC::Math)
            .with_inputs(vec![
                inp("value", "Value", Any),
                inp("min", "Min", Any),
                inp("max", "Max", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.remap", "Remap", NC::Math)
            .with_description("Remap value from input range to output range")
            .with_inputs(vec![
                inp("value", "Value", Any),
                inp("in_min", "In Min", Any),
                inp("in_max", "In Max", Any),
                inp("out_min", "Out Min", Any),
                inp("out_max", "Out Max", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        // --- Vector Math ---
        node("math.dot_product", "Dot Product", NC::Math)
            .with_inputs(vec![inp("a", "A", Vector), inp("b", "B", Vector)])
            .with_outputs(vec![out("result", "Result", Scalar)]),
        node("math.cross_product", "Cross Product", NC::Math)
            .with_inputs(vec![inp("a", "A", Vector), inp("b", "B", Vector)])
            .with_outputs(vec![out("result", "Result", Vector)]),
        node("math.normalize", "Normalize", NC::Math)
            .with_inputs(vec![inp("vector", "Vector", Vector)])
            .with_outputs(vec![out("result", "Result", Vector)]),
        // ==================== Logic ====================
        node("logic.switch", "Switch", NC::Logic)
            .with_inputs(vec![
                inp("condition", "Condition", Boolean),
                inp("true_val", "True", Any),
                inp("false_val", "False", Any),
            ])
            .with_outputs(vec![out("output", "Output", Any)]),
        node("logic.make_list", "Make List", NC::Logic)
            .with_description("Create a list from inputs")
            .with_inputs(vec![inp("item", "Item", Any)])
            .with_outputs(vec![out("list", "List", List)]),
        node("logic.get_list_item", "Get List Item", NC::Logic)
            .with_inputs(vec![
                inp("list", "List", List),
                inp("index", "Index", Integer),
            ])
            .with_outputs(vec![out("item", "Item", Any)]),
        node("logic.list_length", "List Length", NC::Logic)
            .with_inputs(vec![inp("list", "List", List)])
            .with_outputs(vec![out("length", "Length", Integer)]),
        // ==================== Compositing ====================
        node("compositing.transform", "Transform", NC::Compositing)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("position", "Position", Vec2),
                inp("rotation", "Rotation", Scalar),
                inp("scale", "Scale", Vec2),
                inp("anchor", "Anchor", Vec2),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("compositing.composite", "Composite", NC::Compositing)
            .with_description("Blend N images with individual blend modes")
            .with_inputs(vec![
                inp("layers", "Layers", List),
                inp("blend_modes", "Blend Modes", List),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("compositing.normal_blend", "Normal Blend", NC::Compositing)
            .with_inputs(vec![
                inp("foreground", "Foreground", Image),
                inp("background", "Background", Image),
                inp("opacity", "Opacity", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "compositing.multiply_blend",
            "Multiply Blend",
            NC::Compositing,
        )
        .with_inputs(vec![
            inp("foreground", "Foreground", Image),
            inp("background", "Background", Image),
            inp("opacity", "Opacity", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        node("compositing.screen_blend", "Screen Blend", NC::Compositing)
            .with_inputs(vec![
                inp("foreground", "Foreground", Image),
                inp("background", "Background", Image),
                inp("opacity", "Opacity", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "compositing.overlay_blend",
            "Overlay Blend",
            NC::Compositing,
        )
        .with_inputs(vec![
            inp("foreground", "Foreground", Image),
            inp("background", "Background", Image),
            inp("opacity", "Opacity", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        node("compositing.mask", "Mask", NC::Compositing)
            .with_inputs(vec![
                inp("source", "Source", Image),
                inp("mask", "Mask", Image),
                inp("mode", "Mode", Enum),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== Color ====================
        node("color.color_correction", "Color Correction", NC::Color)
            .with_description("Lift, Gamma, Gain 3-way color correction")
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("lift", "Lift", Color),
                inp("gamma", "Gamma", Color),
                inp("gain", "Gain", Color),
                inp("offset", "Offset", Color),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "color.brightness_contrast",
            "Brightness Contrast",
            NC::Color,
        )
        .with_inputs(vec![
            inp("image", "Image", Image),
            inp("brightness", "Brightness", Scalar),
            inp("contrast", "Contrast", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        node("color.hue_saturation", "Hue Saturation", NC::Color)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("hue", "Hue", Scalar),
                inp("saturation", "Saturation", Scalar),
                inp("lightness", "Lightness", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("color.levels", "Levels", NC::Color)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("black_in", "Black In", Scalar),
                inp("white_in", "White In", Scalar),
                inp("gamma", "Gamma", Scalar),
                inp("black_out", "Black Out", Scalar),
                inp("white_out", "White Out", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("color.invert", "Invert", NC::Color)
            .with_inputs(vec![inp("image", "Image", Image)])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("color.hue_shift", "Hue Shift", NC::Color)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("shift_degrees", "Shift Degrees", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("color.color_map", "Color Map", NC::Color)
            .with_description("Map luminance to a color gradient")
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("gradient", "Gradient", Gradient),
                inp("gradient_map", "Gradient Map", Image),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== Generators ====================
        node("generators.solid_color", "Solid Color", NC::Generator)
            .with_inputs(vec![inp("color", "Color", Color)])
            .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "generators.linear_gradient",
            "Linear Gradient",
            NC::Generator,
        )
        .with_inputs(vec![
            inp("start_point", "Start Point", Vec2),
            inp("end_point", "End Point", Vec2),
            inp("gradient", "Gradient", Gradient),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "generators.radial_gradient",
            "Radial Gradient",
            NC::Generator,
        )
        .with_inputs(vec![
            inp("center", "Center", Vec2),
            inp("radius", "Radius", Scalar),
            inp("gradient", "Gradient", Gradient),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "generators.construct_gradient",
            "Construct Gradient",
            NC::Generator,
        )
        .with_description("Create a gradient from color stops")
        .with_inputs(vec![
            inp("colors", "Colors", List),
            inp("positions", "Positions", List),
        ])
        .with_outputs(vec![out("gradient", "Gradient", Gradient)]),
        node(
            "generators.sample_gradient",
            "Sample Gradient",
            NC::Generator,
        )
        .with_inputs(vec![
            inp("gradient", "Gradient", Gradient),
            inp("time", "Time", Scalar),
        ])
        .with_outputs(vec![out("color", "Color", Color)]),
        node("generators.evaluate_curve", "Evaluate Curve", NC::Generator)
            .with_inputs(vec![
                inp("curve", "Curve", Curve),
                inp("time", "Time", Scalar),
            ])
            .with_outputs(vec![out("value", "Value", Scalar)]),
        node("generators.noise", "Noise", NC::Generator)
            .with_inputs(vec![
                inp("scale", "Scale", Scalar),
                inp("seed", "Seed", Integer),
                inp("evolution", "Evolution", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== Filters ====================
        node("filters.blur", "Blur", NC::Filters)
            .with_description("Gaussian blur")
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("radius_x", "Radius X", Scalar),
                inp("radius_y", "Radius Y", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("filters.glow", "Glow", NC::Filters)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("threshold", "Threshold", Scalar),
                inp("radius", "Radius", Scalar),
                inp("intensity", "Intensity", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("filters.drop_shadow", "Drop Shadow", NC::Filters)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("color", "Color", Color),
                inp("distance", "Distance", Scalar),
                inp("angle", "Angle", Scalar),
                inp("softness", "Softness", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== Distortion ====================
        node(
            "distortion.displacement_map",
            "Displacement Map",
            NC::Distortion,
        )
        .with_inputs(vec![
            inp("image", "Image", Image),
            inp("map", "Map", Image),
            inp("scale_x", "Scale X", Scalar),
            inp("scale_y", "Scale Y", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== Path ====================
        node("path.offset_path", "Offset Path", NC::Path)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("offset", "Offset", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.fill_path", "Fill Path", NC::Path)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("color", "Color", Color),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("path.stroke_path", "Stroke Path", NC::Path)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("color", "Color", Color),
                inp("width", "Width", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("path.union_path", "Union Path", NC::Path)
            .with_inputs(vec![inp("paths", "Paths", List)])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.trim_path", "Trim Path", NC::Path)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("start", "Start", Scalar),
                inp("end", "End", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.corner_path", "Corner Path", NC::Path)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("radius", "Radius", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.discrete_path", "Discrete Path", NC::Path)
            .with_description("Jitter the path (DiscretePathEffect)")
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("segment_length", "Segment Length", Scalar),
                inp("deviation", "Deviation", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.simplify_path", "Simplify Path", NC::Path)
            .with_description("Reduce points in path")
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("tolerance", "Tolerance", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        // ==================== Time ====================
        node("time.time_shift", "Time Shift", NC::Time)
            .with_inputs(vec![
                inp("image", "Image", Image),
                inp("time_offset", "Time Offset", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== Image ====================
        node("image.channel_split", "Channel Split", NC::Image)
            .with_inputs(vec![inp("image", "Image", Image)])
            .with_outputs(vec![
                out("r", "R", Image),
                out("g", "G", Image),
                out("b", "B", Image),
                out("a", "A", Image),
            ]),
        node("image.channel_combine", "Channel Combine", NC::Image)
            .with_inputs(vec![
                inp("r", "R", Image),
                inp("g", "G", Image),
                inp("b", "B", Image),
                inp("a", "A", Image),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("image.rgb_to_yuv", "RGB to YUV", NC::Image)
            .with_inputs(vec![inp("rgb_image", "RGB Image", Image)])
            .with_outputs(vec![out("yuv_image", "YUV Image", Image)]),
        node("image.yuv_to_rgb", "YUV to RGB", NC::Image)
            .with_inputs(vec![inp("yuv_image", "YUV Image", Image)])
            .with_outputs(vec![out("rgb_image", "RGB Image", Image)]),
        // ==================== Particles ====================
        node(
            "particles.particle_emitter",
            "Particle Emitter",
            NC::Particles,
        )
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
        node("particles.spawn_burst", "Spawn Burst", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("count", "Count", Integer),
                inp("time", "Time", Scalar),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.shape_location", "Shape Location", NC::Particles)
            .with_description("Initialize particle positions on a shape")
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("shape", "Shape", Enum),
                inp("radius", "Radius", Scalar),
                inp("size", "Size", Vec3),
                inp("surface_only", "Surface Only", Boolean),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node(
            "particles.initialize_particle",
            "Initialize Particle",
            NC::Particles,
        )
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("velocity_min", "Velocity Min", Vec3),
            inp("velocity_max", "Velocity Max", Vec3),
            inp("color_min", "Color Min", Color),
            inp("color_max", "Color Max", Color),
            inp("size_min", "Size Min", Scalar),
            inp("size_max", "Size Max", Scalar),
        ])
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.set_attribute", "Set Attribute", NC::Particles)
            .with_description("Set custom particle attribute (GPU Buffer Write)")
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("attribute_name", "Attribute Name", String),
                inp("value", "Value", Any),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.gravity_force", "Gravity Force", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("force", "Force", Vec3),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.drag_force", "Drag Force", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("coefficient", "Coefficient", Scalar),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.point_force", "Point Force", NC::Particles)
            .with_description("Attract or repel particles")
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("target", "Target", Vec3),
                inp("strength", "Strength", Scalar),
                inp("radius", "Radius", Scalar),
                inp("falloff", "Falloff", Scalar),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.vortex_force", "Vortex Force", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("axis", "Axis", Vec3),
                inp("strength", "Strength", Scalar),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node(
            "particles.vector_field_force",
            "Vector Field Force",
            NC::Particles,
        )
        .with_description("3D Vector Field interaction (GPU Texture 3D)")
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("vector_field", "Vector Field", Any),
            inp("intensity", "Intensity", Scalar),
            inp("tiling", "Tiling", Vec3),
        ])
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.turbulence", "Turbulence", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("frequency", "Frequency", Scalar),
                inp("strength", "Strength", Scalar),
                inp("octave", "Octave", Integer),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node(
            "particles.color_over_life",
            "Color Over Life",
            NC::Particles,
        )
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("gradient", "Gradient", Gradient),
        ])
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node("particles.size_over_life", "Size Over Life", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("curve", "Curve", Curve),
            ])
            .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node(
            "particles.collision_plane",
            "Collision Plane",
            NC::Particles,
        )
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("plane_point", "Plane Point", Vec3),
            inp("plane_normal", "Plane Normal", Vec3),
            inp("bounce", "Bounce", Scalar),
            inp("friction", "Friction", Scalar),
        ])
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node(
            "particles.collision_depth",
            "Collision Depth",
            NC::Particles,
        )
        .with_description("Screen-space depth collision (GPU)")
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("depth_buffer", "Depth Buffer", Image),
            inp("thickness", "Thickness", Scalar),
        ])
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)]),
        node(
            "particles.sprite_renderer",
            "Sprite Renderer",
            NC::Particles,
        )
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("texture", "Texture", Image),
            inp("color", "Color", Color),
            inp("blend_mode", "Blend Mode", Enum),
            inp("alignment", "Alignment", Enum),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        node("particles.mesh_renderer", "Mesh Renderer", NC::Particles)
            .with_inputs(vec![
                inp("particles", "Particles", ParticleSystem),
                inp("mesh", "Mesh", Any),
                inp("material", "Material", Material),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node(
            "particles.ribbon_renderer",
            "Ribbon Renderer",
            NC::Particles,
        )
        .with_description("Connect particles with a trail")
        .with_inputs(vec![
            inp("particles", "Particles", ParticleSystem),
            inp("texture", "Texture", Image),
            inp("width", "Width", Scalar),
            inp("max_trail_length", "Max Trail Length", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)]),
        // ==================== 3D ====================
        node("3d.camera_3d", "Camera 3D", NC::ThreeD)
            .with_inputs(vec![
                inp("position", "Position", Vec3),
                inp("target", "Target", Vec3),
                inp("up", "Up", Vec3),
                inp("fov", "FOV", Scalar),
            ])
            .with_outputs(vec![out("camera", "Camera", Camera3D)]),
        node("3d.transform_3d", "Transform 3D", NC::ThreeD)
            .with_inputs(vec![
                inp("object", "Object", Object3D),
                inp("translation", "Translation", Vec3),
                inp("rotation", "Rotation", Vec3),
                inp("scale", "Scale", Vec3),
            ])
            .with_outputs(vec![out("object", "Object", Object3D)]),
        node("3d.mesh_instance", "Mesh Instance", NC::ThreeD)
            .with_inputs(vec![
                inp("mesh_asset", "Mesh Asset", Any),
                inp("material", "Material", Material),
            ])
            .with_outputs(vec![out("object", "Object", Object3D)]),
        node("3d.render_3d", "Render 3D", NC::ThreeD)
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
