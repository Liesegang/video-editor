use eframe::egui;
use library::model::project::{
    PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner, MERGE_IMAGES_PORT,
};
use library::model::{BlendMode, NodeContent, Project};
use std::collections::HashMap;
use uuid::Uuid;

#[cfg(test)]
use crate::ui::panels::node_editor::{capture_test_metadata, capture_test_rect};
use crate::ui::panels::node_editor::{clipped_qa_rect, qa_container_key, qa_rect_metadata};

pub(in crate::ui::panels::node_editor) const AUTHORED_BLEND_MODES: [BlendMode; 5] = [
    BlendMode::Normal,
    BlendMode::Add,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
];

pub(in crate::ui::panels::node_editor) fn blend_mode_label(blend_mode: BlendMode) -> &'static str {
    match blend_mode {
        BlendMode::Normal => "Normal",
        BlendMode::Add => "Add",
        BlendMode::Multiply => "Multiply",
        BlendMode::Screen => "Screen",
        BlendMode::Overlay => "Overlay",
    }
}

pub(in crate::ui::panels::node_editor) fn blend_mode_qa_key(blend_mode: BlendMode) -> &'static str {
    match blend_mode {
        BlendMode::Normal => "normal",
        BlendMode::Add => "add",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct MergeLayerRow {
    pub(in crate::ui::panels::node_editor) merge_id: Uuid,
    pub(in crate::ui::panels::node_editor) connection_id: Uuid,
    pub(in crate::ui::panels::node_editor) source: PortAddress,
    pub(in crate::ui::panels::node_editor) source_label: String,
    pub(in crate::ui::panels::node_editor) authored_order: i64,
    pub(in crate::ui::panels::node_editor) authored_blend_mode: BlendMode,
    pub(in crate::ui::panels::node_editor) authored_blend_available: bool,
    pub(in crate::ui::panels::node_editor) back_to_front_index: usize,
    pub(in crate::ui::panels::node_editor) layer_count: usize,
}

impl MergeLayerRow {
    pub(in crate::ui::panels::node_editor) fn qa_metadata(
        &self,
        extra: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let source_kind = match self.source.owner {
            PortOwner::Composition(_) => "composition",
            PortOwner::Track(_) => "track",
            PortOwner::Clip(_) => "clip",
            PortOwner::Node(_) => "node",
        };
        let mut metadata = serde_json::json!({
            "merge_id": self.merge_id,
            "connection_id": self.connection_id,
            "back_to_front_index": self.back_to_front_index,
            "layer_count": self.layer_count,
            "authored_order": self.authored_order,
            "authored_blend_mode": blend_mode_qa_key(self.authored_blend_mode),
            "authored_blend_available": self.authored_blend_available,
            "source": {
                "owner": qa_container_key(self.source.owner),
                "kind": source_kind,
                "port": self.source.port,
                "label": self.source_label,
                "full_name_available_on_hover": true,
            },
            "order_semantics": "back_to_front",
            "blend_ownership": "connection",
            "control_lane": "merge_body",
            "runtime_first_produced_may_be_normal": true,
        });
        if let (Some(target), Some(serde_json::Value::Object(extra))) =
            (metadata.as_object_mut(), extra)
        {
            target.extend(extra);
        }
        metadata
    }
}

fn merge_layer_source_label(project: &Project, owner: PortOwner) -> String {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| format!("Composition · {}", composition.name))
            .unwrap_or_else(|| "Missing Composition".to_string()),
        PortOwner::Track(id) => project
            .get_track(id)
            .map(|track| format!("Track · {}", track.name))
            .unwrap_or_else(|| "Missing Track".to_string()),
        PortOwner::Clip(id) => project
            .get_clip(id)
            .map(|clip| format!("Clip · {}", clip.name))
            .unwrap_or_else(|| "Missing Clip".to_string()),
        PortOwner::Node(id) => project
            .get_node(id)
            .map(|node| format!("Node · {}", node.name))
            .unwrap_or_else(|| "Missing Node".to_string()),
    }
}

pub(in crate::ui::panels::node_editor) fn merge_layer_rows(
    project: &Project,
    merge_id: Uuid,
) -> Vec<MergeLayerRow> {
    let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    let layer_count = connections.len();
    connections
        .into_iter()
        .enumerate()
        .map(|(back_to_front_index, connection)| MergeLayerRow {
            merge_id,
            connection_id: connection.id,
            source: connection.from.clone(),
            source_label: merge_layer_source_label(project, connection.from.owner),
            authored_order: connection.order,
            authored_blend_mode: connection.blend_mode,
            authored_blend_available: connection_supports_authored_blend(project, connection),
            back_to_front_index,
            layer_count,
        })
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "QA registration keeps semantic identity and transformed geometry explicit"
)]
pub(in crate::ui::panels::node_editor) fn register_merge_layer_component(
    id: String,
    component_type: &str,
    graph_rect: egui::Rect,
    enabled: bool,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    mut metadata: serde_json::Value,
) {
    let unclipped_rect = to_global * graph_rect;
    let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert(
            "unclipped_rect".to_string(),
            qa_rect_metadata(unclipped_rect),
        );
        metadata.insert(
            "visible_in_canvas".to_string(),
            serde_json::Value::Bool(rect.is_positive()),
        );
    }
    #[cfg(test)]
    capture_test_rect(&id, rect);
    crate::qa::register_component_with_metadata(id, component_type, rect, enabled, Some(metadata));
}

pub(in crate::ui::panels::node_editor) fn register_merge_layer_popup_component(
    id: String,
    component_type: &str,
    screen_rect: egui::Rect,
    enabled: bool,
    popup_clip: egui::Rect,
    mut metadata: serde_json::Value,
) {
    let rect = clipped_qa_rect(screen_rect, popup_clip);
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert("unclipped_rect".to_string(), qa_rect_metadata(screen_rect));
        metadata.insert("popup_clip_rect".to_string(), qa_rect_metadata(popup_clip));
        metadata.insert(
            "visible_in_popup".to_string(),
            serde_json::Value::Bool(rect.is_positive()),
        );
        metadata.insert(
            "coordinate_space".to_string(),
            serde_json::Value::String("screen_points".to_string()),
        );
    }
    #[cfg(test)]
    {
        capture_test_rect(&id, rect);
        capture_test_metadata(&id, &metadata);
    }
    crate::qa::register_component_with_metadata(id, component_type, rect, enabled, Some(metadata));
}

pub(in crate::ui::panels::node_editor) fn connection_supports_authored_blend(
    project: &Project,
    connection: &library::model::project::ProjectConnection,
) -> bool {
    let source_is_image = project
        .port_definition(&connection.from, PortDirection::Output)
        .is_some_and(|definition| definition.data_type == PortDataType::Image);
    let target_is_merge_images = connection.to.port == library::model::project::MERGE_IMAGES_PORT
        && matches!(
            connection.to.owner,
            PortOwner::Node(node_id)
                if project
                    .get_node(node_id)
                    .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
        )
        && project
            .port_definition(&connection.to, PortDirection::Input)
            .is_some_and(|definition| {
                definition.data_type == PortDataType::Image
                    && definition.multiplicity == PortMultiplicity::Variadic
            });
    source_is_image && target_is_merge_images
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct WireOrderMenuState {
    pub(in crate::ui::panels::node_editor) back_to_front_index: usize,
    pub(in crate::ui::panels::node_editor) layer_count: usize,
}

pub(in crate::ui::panels::node_editor) fn wire_order_menu_state(
    project: &Project,
    connection: &library::model::project::ProjectConnection,
) -> Option<WireOrderMenuState> {
    wire_order_menu_states(project).get(&connection.id).copied()
}

pub(in crate::ui::panels::node_editor) fn wire_order_menu_states(
    project: &Project,
) -> HashMap<Uuid, WireOrderMenuState> {
    let mut groups = HashMap::<PortAddress, Vec<(i64, Uuid)>>::new();
    for connection in &project.connections {
        let is_variadic = project
            .port_definition(&connection.to, PortDirection::Input)
            .is_some_and(|definition| definition.multiplicity == PortMultiplicity::Variadic);
        if is_variadic {
            groups
                .entry(connection.to.clone())
                .or_default()
                .push((connection.order, connection.id));
        }
    }

    let mut states = HashMap::new();
    for siblings in groups.values_mut() {
        siblings.sort_by_key(|(order, id)| (*order, *id));
        let layer_count = siblings.len();
        for (back_to_front_index, (_, connection_id)) in siblings.iter().enumerate() {
            states.insert(
                *connection_id,
                WireOrderMenuState {
                    back_to_front_index,
                    layer_count,
                },
            );
        }
    }
    states
}

pub(in crate::ui::panels::node_editor) fn wire_order_qa_metadata(
    connection: &library::model::project::ProjectConnection,
    order: WireOrderMenuState,
    direction: &str,
    target_index: Option<usize>,
) -> serde_json::Value {
    serde_json::json!({
        "action": "reorder",
        "connection_id": connection.id,
        "direction": direction,
        "back_to_front_index": order.back_to_front_index,
        "authored_order": connection.order,
        "layer_count": order.layer_count,
        "target_back_to_front_index": target_index,
        "authored_blend_mode": blend_mode_qa_key(connection.blend_mode),
    })
}
