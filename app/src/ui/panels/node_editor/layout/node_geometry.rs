use eframe::egui;
use library::model::{GeneratorContent, Node, NodeContent, Project};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    input_definitions, merge_layer_rows, output_definitions, GraphItem, COMPOSE_COLOR_BODY_WIDTH,
    MERGE_BODY_WIDTH, NODE_BODY_WIDTH, NODE_HEADER_WIDTH, PORT_LABEL_WIDTH, PORT_ROW_HEIGHT,
};

/// Conservative non-body allowance for the input controls, output label,
/// pin sockets, frame margins, and inter-lane spacing used by Snarl's Coil
/// layout. Compose has a compact aggregate body, but its five authored input
/// rows still determine the wider input lane.
const NODE_HORIZONTAL_CHROME_WIDTH: f32 = 70.0;
const COMPOSE_COLOR_HORIZONTAL_CHROME_WIDTH: f32 = 224.0;

pub(in crate::ui::panels::node_editor) fn estimated_node_width() -> f32 {
    estimated_node_width_for_body(NODE_BODY_WIDTH, NODE_HORIZONTAL_CHROME_WIDTH)
}

fn estimated_node_width_for_body(body_width: f32, horizontal_chrome_width: f32) -> f32 {
    (body_width + PORT_LABEL_WIDTH * 2.0 + horizontal_chrome_width).max(NODE_HEADER_WIDTH + 30.0)
}

pub(in crate::ui::panels::node_editor) const fn node_body_width(content: &NodeContent) -> f32 {
    if matches!(
        content,
        NodeContent::Color(library::model::ColorContent::Compose)
    ) {
        COMPOSE_COLOR_BODY_WIDTH
    } else {
        NODE_BODY_WIDTH
    }
}

fn estimated_node_width_for_content(content: &NodeContent) -> f32 {
    if matches!(
        content,
        NodeContent::Color(library::model::ColorContent::Compose)
    ) {
        estimated_node_width_for_body(
            COMPOSE_COLOR_BODY_WIDTH,
            COMPOSE_COLOR_HORIZONTAL_CHROME_WIDTH,
        )
    } else {
        estimated_node_width()
    }
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
            | NodeContent::Color(_)
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
            content.map_or_else(estimated_node_width, estimated_node_width_for_content)
        },
        base_height + pin_rows.saturating_sub(4) as f32 * PORT_ROW_HEIGHT,
    )
}
