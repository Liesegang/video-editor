//! Shared production Node Editor chrome.

mod property_controls;
mod theme;

pub(in crate::ui::panels::node_editor) use property_controls::{
    measured_label_width, property_label,
};
pub(in crate::ui::panels::node_editor) use theme::{
    node_icon_for_node, node_palette_for_node, pin_info,
};
