//! Theming for the node editor.

use egui::Color32;

/// Theme configuration for the node editor.
pub struct NodeEditorTheme {
    /// Pin color based on type_id string.
    pub pin_color: Box<dyn Fn(&str) -> Color32>,
    /// Header color based on type_id string.
    pub header_color: Box<dyn Fn(&str) -> Color32>,
    /// Node width in pixels.
    pub node_width: f32,
    /// Header height in pixels.
    pub header_height: f32,
    /// Pin row height in pixels.
    pub pin_row_height: f32,
    /// Pin circle radius.
    pub pin_radius: f32,
    /// Pin margin from node edge.
    pub pin_margin: f32,
    /// Corner rounding for nodes.
    pub node_rounding: f32,
    /// Background color.
    pub background_color: Color32,
    /// Grid line color.
    pub grid_color: Color32,
    /// Grid spacing.
    pub grid_spacing: f32,
    /// Node body color (unselected).
    pub node_body_color: Color32,
    /// Node body color (selected).
    pub node_body_selected_color: Color32,
    /// Selection outline color.
    pub selection_color: Color32,
    /// Pin label color.
    pub pin_label_color: Color32,
    /// Connection color (default).
    pub connection_color: Color32,
    /// Connection color (selected).
    pub connection_selected_color: Color32,
}

impl Default for NodeEditorTheme {
    fn default() -> Self {
        Self {
            pin_color: Box::new(default_pin_color),
            header_color: Box::new(default_header_color),
            node_width: 180.0,
            header_height: 24.0,
            pin_row_height: 20.0,
            pin_radius: 5.0,
            pin_margin: 12.0,
            node_rounding: 4.0,
            background_color: Color32::from_rgb(30, 30, 30),
            grid_color: Color32::from_rgb(40, 40, 40),
            grid_spacing: 50.0,
            node_body_color: Color32::from_rgb(45, 45, 50),
            node_body_selected_color: Color32::from_rgb(55, 55, 65),
            selection_color: Color32::from_rgb(100, 150, 255),
            pin_label_color: Color32::from_rgb(200, 200, 200),
            connection_color: Color32::from_rgb(180, 180, 180),
            connection_selected_color: Color32::WHITE,
        }
    }
}

fn default_pin_color(type_id: &str) -> Color32 {
    if type_id.starts_with("effect.") || type_id.starts_with("filters.") {
        Color32::from_rgb(238, 207, 109) // Yellow
    } else if type_id.starts_with("style.") {
        Color32::from_rgb(109, 238, 150) // Green
    } else if type_id.starts_with("effector.") {
        Color32::from_rgb(180, 109, 238) // Purple
    } else if type_id.starts_with("decorator.") {
        Color32::from_rgb(238, 130, 109) // Orange
    } else if type_id.starts_with("math.") {
        Color32::from_rgb(109, 200, 238) // Cyan
    } else if type_id.starts_with("color.") {
        Color32::from_rgb(150, 238, 120) // Light green
    } else if type_id.starts_with("compositing.") {
        Color32::from_rgb(238, 180, 109) // Light orange
    } else if type_id.starts_with("data.") {
        Color32::from_rgb(160, 160, 200) // Light blue-grey
    } else if type_id.starts_with("generators.") {
        Color32::from_rgb(200, 140, 238) // Light purple
    } else if type_id.starts_with("particles.") {
        Color32::from_rgb(238, 109, 130) // Red-pink
    } else if type_id.starts_with("3d.") {
        Color32::from_rgb(109, 140, 238) // Blue
    } else if type_id.starts_with("path.") {
        Color32::from_rgb(238, 170, 109) // Orange
    } else if type_id.starts_with("text.") {
        Color32::from_rgb(109, 238, 200) // Teal
    } else if type_id.starts_with("logic.") {
        Color32::from_rgb(238, 238, 109) // Yellow-green
    } else if type_id.starts_with("image.") {
        Color32::from_rgb(238, 200, 150) // Peach
    } else if type_id.starts_with("time.") || type_id.starts_with("scripting.") {
        Color32::from_rgb(200, 200, 120) // Khaki
    } else {
        Color32::from_rgb(150, 150, 150) // Grey
    }
}

fn default_header_color(type_id: &str) -> Color32 {
    if type_id.starts_with("effect.") || type_id.starts_with("filters.") {
        Color32::from_rgb(60, 100, 160)
    } else if type_id.starts_with("style.") {
        Color32::from_rgb(60, 130, 80)
    } else if type_id.starts_with("effector.") {
        Color32::from_rgb(100, 60, 150)
    } else if type_id.starts_with("decorator.") {
        Color32::from_rgb(150, 80, 60)
    } else if type_id.starts_with("math.") {
        Color32::from_rgb(50, 100, 130)
    } else if type_id.starts_with("color.") {
        Color32::from_rgb(60, 120, 60)
    } else if type_id.starts_with("compositing.") {
        Color32::from_rgb(130, 90, 50)
    } else if type_id.starts_with("data.") {
        Color32::from_rgb(70, 70, 100)
    } else if type_id.starts_with("generators.") {
        Color32::from_rgb(100, 60, 130)
    } else if type_id.starts_with("particles.") {
        Color32::from_rgb(130, 50, 60)
    } else if type_id.starts_with("3d.") {
        Color32::from_rgb(50, 60, 130)
    } else if type_id.starts_with("path.") {
        Color32::from_rgb(130, 90, 50)
    } else if type_id.starts_with("text.") {
        Color32::from_rgb(50, 120, 100)
    } else if type_id.starts_with("logic.") {
        Color32::from_rgb(110, 110, 50)
    } else if type_id.starts_with("image.") {
        Color32::from_rgb(120, 90, 60)
    } else if type_id.starts_with("time.") || type_id.starts_with("scripting.") {
        Color32::from_rgb(100, 100, 60)
    } else if type_id.starts_with("track") {
        Color32::from_rgb(60, 80, 60)
    } else if type_id.starts_with("clip.") {
        Color32::from_rgb(80, 70, 90)
    } else {
        Color32::from_rgb(80, 80, 80)
    }
}
