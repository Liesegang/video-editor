use eframe::egui;
use library::model::{GeneratorContent, Node, NodeContent, Project};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    input_definitions, merge_layer_rows, output_definitions, GraphItem, MERGE_BODY_WIDTH,
    NODE_BODY_WIDTH, NODE_HEADER_WIDTH, PORT_LABEL_WIDTH, PORT_ROW_HEIGHT,
};

pub(in crate::ui::panels::node_editor) fn estimated_node_width() -> f32 {
    (NODE_BODY_WIDTH + PORT_LABEL_WIDTH * 2.0 + 70.0).max(NODE_HEADER_WIDTH + 30.0)
}

/// Conservative allowance for Snarl's pin lanes, frame margins, and stroke
/// around the shared physical Merge body. Keeping this in the sole Merge
/// width function prevents layout and test helpers from drifting apart.
const MERGE_HORIZONTAL_CHROME_WIDTH: f32 = 92.0;

pub(in crate::ui::panels::node_editor) fn estimated_merge_node_width() -> f32 {
    (MERGE_BODY_WIDTH + PORT_LABEL_WIDTH * 2.0 + MERGE_HORIZONTAL_CHROME_WIDTH)
        .max(NODE_HEADER_WIDTH + 30.0)
}

pub(in crate::ui::panels::node_editor) fn estimated_node_size(
    project: &Project,
    node_id: Uuid,
) -> egui::Vec2 {
    let item = GraphItem::Node(node_id);
    let pin_rows = input_definitions(project, item)
        .len()
        .max(output_definitions(project, item).len());
    let content = project.get_node(node_id).map(Node::content);
    let base_height = match content {
        Some(NodeContent::Generator(GeneratorContent::Text)) => 330.0,
        Some(NodeContent::Generator(GeneratorContent::Shape))
        | Some(NodeContent::Generator(GeneratorContent::SkSL)) => 300.0,
        Some(NodeContent::Generator(GeneratorContent::Solid)) => 240.0,
        Some(NodeContent::PluginOperation(_)) => 260.0,
        Some(NodeContent::NativeOperation(_)) => 260.0,
        Some(
            NodeContent::Merge
            | NodeContent::SoundMerge
            | NodeContent::List(library::model::ListContent::Make),
        ) => {
            let layer_count = merge_layer_rows(project, node_id).len();
            (166.0 + layer_count as f32 * 82.0).max(220.0)
        }
        Some(NodeContent::SoundAnalysis(_)) => 260.0,
        Some(
            NodeContent::Media(_)
            | NodeContent::CompositionInstance(_)
            | NodeContent::Value(_)
            | NodeContent::Data(_)
            | NodeContent::Path(_)
            | NodeContent::List(_),
        ) => 220.0,
        None => 220.0,
    };
    egui::vec2(
        if content.is_some_and(|content| {
            matches!(
                content,
                NodeContent::Merge
                    | NodeContent::SoundMerge
                    | NodeContent::List(library::model::ListContent::Make)
            )
        }) {
            estimated_merge_node_width()
        } else {
            estimated_node_width()
        },
        base_height + pin_rows.saturating_sub(4) as f32 * PORT_ROW_HEIGHT,
    )
}
