use egui::Ui;
use library::model::{Node, NodeContent};
use library::plugin::PATH_EFFECT_CATEGORY;

pub(super) const CATEGORY_SECTION: (&str, &str, &str) =
    (PATH_EFFECT_CATEGORY, "Path Effects", "Path geometry only");
pub(super) const SUPPORTED_GEOMETRY: &str = "path_only";
pub(super) const UNSUPPORTED_GEOMETRY: &str = "text";

pub(super) fn is_category(category: &str) -> bool {
    category == PATH_EFFECT_CATEGORY
}

pub(super) fn is_node(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation) if is_category(&operation.category)
    )
}

pub(super) fn render_contract(ui: &mut Ui, node: &Node) {
    if !is_node(node) {
        return;
    }
    let response = ui.label(
        egui::RichText::new(
            "Path geometry only. Text requires an explicit outline-extraction operation.",
        )
        .small()
        .weak(),
    );
    crate::qa::register_component_with_metadata(
        format!("inspector.path_effect_contract:{}", node.id),
        "inspector_operation_contract",
        response.rect,
        true,
        Some(serde_json::json!({
            "operation_id": node.id,
            "category": PATH_EFFECT_CATEGORY,
            "shape_geometry": SUPPORTED_GEOMETRY,
            "unsupported_shape_geometry": UNSUPPORTED_GEOMETRY,
        })),
    );
}
