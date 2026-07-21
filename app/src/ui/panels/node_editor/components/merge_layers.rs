use eframe::egui;
use library::model::project::{
    PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner, MERGE_IMAGES_PORT,
};
use library::model::{BlendMode, NodeContent, Project};
use std::collections::HashMap;
use uuid::Uuid;

use crate::ui::layer_order::reverse_index;
#[cfg(test)]
use crate::ui::panels::node_editor::capture_test_rect;
use crate::ui::panels::node_editor::{
    canonical_pin_definitions, clipped_qa_rect, qa_container_key, qa_rect_metadata, PinDefinition,
};
use crate::ui::widgets::searchable_context_menu::SearchableItem;

const AUTHORED_BLEND_MODES: [BlendMode; 29] = BlendMode::ALL;

pub(in crate::ui::panels::node_editor) fn blend_mode_label(blend_mode: BlendMode) -> &'static str {
    blend_mode.label()
}

pub(in crate::ui::panels::node_editor) fn blend_mode_qa_key(blend_mode: BlendMode) -> &'static str {
    blend_mode.qa_key()
}

pub(in crate::ui::panels::node_editor) fn blend_mode_searchable_items(
    selected: BlendMode,
) -> Vec<SearchableItem<BlendMode>> {
    AUTHORED_BLEND_MODES
        .into_iter()
        .map(|blend_mode| {
            let mut item = SearchableItem::new(blend_mode.label(), blend_mode);
            item.category = Some(blend_mode.group().label().to_string());
            item.keywords = vec![
                blend_mode.qa_key().to_string(),
                blend_mode.group().qa_key().to_string(),
            ];
            item.enabled = blend_mode != selected;
            item
        })
        .collect()
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
    /// Stable index in the canonical persisted/render order (back to front).
    pub(in crate::ui::panels::node_editor) back_to_front_index: usize,
    /// Physical row index in the Node Editor (front to back, top to bottom).
    pub(in crate::ui::panels::node_editor) front_to_back_index: usize,
    pub(in crate::ui::panels::node_editor) layer_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum MergeInputSlotRole {
    Canonical,
    Connected(MergeLayerRow),
    VacantImages,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct MergeInputSlot {
    pub(in crate::ui::panels::node_editor) definition: PinDefinition,
    pub(in crate::ui::panels::node_editor) role: MergeInputSlotRole,
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
            "front_to_back_index": self.front_to_back_index,
            "visual_index": self.front_to_back_index,
            "canonical_index": self.back_to_front_index,
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
            "canonical_order_semantics": "back_to_front",
            "visual_order_semantics": "front_to_back",
            "order_semantics": "back_to_front",
            "blend_ownership": "connection",
            "control_lane": "merge_body",
            "runtime_first_produced_may_be_normal": self
                .authored_blend_mode
                .can_optimize_empty_backdrop_to_normal(),
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
    if merge_images_target_node_id(project, &target).is_none() {
        return Vec::new();
    }
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    let layer_count = connections.len();
    let mut rows = connections
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
            front_to_back_index: 0,
            layer_count,
        })
        .collect::<Vec<_>>();
    rows.reverse();
    for (front_to_back_index, row) in rows.iter_mut().enumerate() {
        debug_assert_eq!(
            reverse_index(row.back_to_front_index, layer_count),
            Some(front_to_back_index)
        );
        row.front_to_back_index = front_to_back_index;
    }
    rows
}

/// Identify the one input whose variadic connections are projected as
/// physical Merge layer slots. A matching port key alone is intentionally
/// insufficient: plugin operations may declare their own variadic Image
/// input named `images`, and those remain ordinary graph pins.
pub(in crate::ui::panels::node_editor) fn merge_images_target_node_id(
    project: &Project,
    target: &PortAddress,
) -> Option<Uuid> {
    let PortOwner::Node(node_id) = target.owner else {
        return None;
    };
    if target.port != MERGE_IMAGES_PORT
        || !project
            .get_node(node_id)
            .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
    {
        return None;
    }
    project
        .port_definition(target, PortDirection::Input)
        .is_some_and(|definition| {
            definition.data_type == PortDataType::Image
                && definition.multiplicity == PortMultiplicity::Variadic
        })
        .then_some(node_id)
}

/// Expand only Merge's variadic `images` definition into one physical input
/// pin per canonical connection plus one vacant back-insertion pin. Connected
/// rows are physically front-to-back while retaining their canonical logical
/// indices. The Project port remains a single variadic address; this is a view
/// projection, not a second graph model.
pub(in crate::ui::panels::node_editor) fn merge_input_slots(
    project: &Project,
    merge_id: Uuid,
) -> Vec<MergeInputSlot> {
    let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let expand_images = merge_images_target_node_id(project, &target).is_some();
    let rows = merge_layer_rows(project, merge_id);
    canonical_pin_definitions(
        project,
        PortOwner::Node(merge_id),
        PortDirection::Input,
        library::model::project::PortSide::Left,
    )
    .into_iter()
    .flat_map(|definition| {
        if definition.key != MERGE_IMAGES_PORT || !expand_images {
            return vec![MergeInputSlot {
                definition,
                role: MergeInputSlotRole::Canonical,
            }];
        }
        let mut slots = rows
            .iter()
            .cloned()
            .map(|row| MergeInputSlot {
                definition: definition.clone(),
                role: MergeInputSlotRole::Connected(row),
            })
            .collect::<Vec<_>>();
        slots.push(MergeInputSlot {
            definition,
            role: MergeInputSlotRole::VacantImages,
        });
        slots
    })
    .collect()
}

pub(in crate::ui::panels::node_editor) fn merge_input_index_for_connection(
    project: &Project,
    merge_id: Uuid,
    connection_id: Uuid,
) -> Option<usize> {
    merge_input_slots(project, merge_id)
        .iter()
        .position(|slot| {
            matches!(
                &slot.role,
                MergeInputSlotRole::Connected(row) if row.connection_id == connection_id
            )
        })
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

pub(in crate::ui::panels::node_editor) fn connection_supports_authored_blend(
    project: &Project,
    connection: &library::model::project::ProjectConnection,
) -> bool {
    let source_is_image = project
        .port_definition(&connection.from, PortDirection::Output)
        .is_some_and(|definition| definition.data_type == PortDataType::Image);
    let target_is_merge_images = merge_images_target_node_id(project, &connection.to).is_some();
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

#[cfg(test)]
mod tests {
    use super::blend_mode_searchable_items;
    use library::model::{BlendMode, BlendModeGroup};

    #[test]
    fn blend_search_catalog_is_complete_grouped_and_has_one_disabled_selection() {
        let items = blend_mode_searchable_items(BlendMode::VividLight);
        assert_eq!(items.len(), 29);
        assert_eq!(
            items.iter().map(|item| item.value).collect::<Vec<_>>(),
            BlendMode::ALL
        );
        assert_eq!(items.iter().filter(|item| !item.enabled).count(), 1);
        for group in BlendModeGroup::ALL {
            assert!(items.iter().any(|item| {
                item.category.as_deref() == Some(group.label())
                    && item
                        .keywords
                        .iter()
                        .any(|keyword| keyword == group.qa_key())
            }));
        }
        let linear_dodge = items
            .iter()
            .find(|item| item.value == BlendMode::LinearDodge)
            .unwrap();
        assert_eq!(linear_dodge.label, "Linear Dodge (Add)");
        assert!(linear_dodge
            .keywords
            .iter()
            .any(|keyword| keyword == "linear_dodge"));
    }
}
