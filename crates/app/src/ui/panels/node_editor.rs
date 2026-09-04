//! The production Node Editor surface.
//!
//! A document supplies processing nodes and applies edit intents; this module
//! owns the shared visual language, navigation policy, and Snarl interaction
//! surface. Timeline items are deliberately outside this module: only an
//! explicitly opened Node Clip supplies a `ModuleDefinition` document.

mod canvas;
mod components;
mod module_document;

pub use module_document::node_editor_panel;

use canvas::{
    node_editor_details_visible, node_editor_navigation_config,
    node_editor_port_interactions_enabled, node_editor_snarl_style_for,
    paint_node_editor_canvas_grid,
};
use components::{
    measured_label_width, node_icon_for_node, node_palette_for_node, pin_info, property_label,
};

// Physical dimensions from the original production Node Editor. Keeping them
// at the surface boundary makes Snarl layout, property controls, and hit boxes
// derive from one contract.
const PORT_SOCKET_SIZE: f32 = 13.0;
const NODE_HEADER_WIDTH: f32 = 190.0;
const PORT_LABEL_WIDTH: f32 = 96.0;
const PORT_ROW_HEIGHT: f32 = 22.0;
const PROPERTY_LABEL_WIDTH: f32 = 58.0;
