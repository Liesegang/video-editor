use eframe::egui;
use library::model::project::connection::{LIST_ITEM_OUTPUT_PORT, LIST_ITEMS_INPUT_PORT};
use library::model::project::{
    AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT, PortAddress,
    PortDataType, PortDirection, PortMultiplicity, PortOwner,
};
use library::model::{BlendMode, ListContent, NodeContainer, NodeContent, Project};
use std::{cmp::Ordering, collections::HashMap};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    PinDefinition, canonical_pin_definitions, clipped_qa_rect, qa_container_key, qa_rect_metadata,
};
#[cfg(test)]
use crate::ui::panels::node_editor::{capture_test_metadata, capture_test_rect};
use crate::ui::widgets::searchable_context_menu::SearchableItem;

const AUTHORED_BLEND_MODES: [BlendMode; 29] = BlendMode::ALL;

/// Native ordered variadic inputs projected as physical rows in the Node
/// Editor. This is presentation metadata over authoritative Project
/// connections, not a second graph model. The legacy type name keeps existing
/// Image/Sound automation IDs stable while List adopts this same contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum NativeVariadicMergeKind {
    Image,
    Sound,
    List,
}

impl NativeVariadicMergeKind {
    pub(in crate::ui::panels::node_editor) const fn input_port(self) -> &'static str {
        match self {
            Self::Image => MERGE_IMAGES_PORT,
            Self::Sound => MERGE_SOUNDS_PORT,
            Self::List => LIST_ITEMS_INPUT_PORT,
        }
    }

    const fn data_type(self) -> PortDataType {
        match self {
            Self::Image => PortDataType::Image,
            Self::Sound => PortDataType::Audio,
            Self::List => PortDataType::Any,
        }
    }

    pub(in crate::ui::panels::node_editor) const fn qa_key(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Sound => "sound",
            Self::List => "list",
        }
    }

    pub(in crate::ui::panels::node_editor) const fn canonical_order_semantics(
        self,
    ) -> &'static str {
        match self {
            Self::Image => "back_to_front",
            Self::Sound | Self::List => "top_to_bottom",
        }
    }

    pub(in crate::ui::panels::node_editor) const fn visual_order_semantics(self) -> &'static str {
        match self {
            Self::Image => "front_to_back",
            Self::Sound | Self::List => "top_to_bottom",
        }
    }

    const fn canonical_index_for_visual(self, visual_index: usize, item_count: usize) -> usize {
        match self {
            Self::Image => item_count - visual_index - 1,
            Self::Sound | Self::List => visual_index,
        }
    }

    fn visual_connection_cmp(self, left: (i64, Uuid), right: (i64, Uuid)) -> Ordering {
        let canonical = left.cmp(&right);
        match self {
            Self::Image => canonical.reverse(),
            Self::Sound | Self::List => canonical,
        }
    }

    const fn vacant_canonical_index(
        self,
        item_count: usize,
        structural_prefix_len: usize,
    ) -> usize {
        match self {
            Self::Image => structural_prefix_len,
            Self::Sound | Self::List => item_count,
        }
    }

    pub(in crate::ui::panels::node_editor) const fn display_name(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Sound => "Sound",
            Self::List => "Item",
        }
    }

    const fn vacant_insertion_semantics(self, structural_prefix_len: usize) -> &'static str {
        match self {
            Self::Image if structural_prefix_len > 0 => "custom_back_after_structural_prefix",
            Self::Image => "back",
            Self::Sound | Self::List => "end",
        }
    }

    const fn has_blend_controls(self) -> bool {
        matches!(self, Self::Image)
    }
}

/// Canonical and physical placement of a native variadic Merge's vacant pin.
///
/// Structural children form a mandatory canonical prefix. Image layers are
/// displayed in the inverse (front-to-back) direction, so their vacant pin is
/// placed at the physical boundary that maps to the first legal custom slot.
/// This keeps the pin's screen position honest while leaving ordinary Image
/// Merge and Sound Merge insertion unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct MergeVacantSlot {
    pub(in crate::ui::panels::node_editor) canonical_index: usize,
    pub(in crate::ui::panels::node_editor) visual_index: usize,
    pub(in crate::ui::panels::node_editor) layer_count: usize,
    pub(in crate::ui::panels::node_editor) structural_prefix_len: usize,
    pub(in crate::ui::panels::node_editor) insertion_semantics: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct NativeVariadicMerge {
    pub(in crate::ui::panels::node_editor) node_id: Uuid,
    pub(in crate::ui::panels::node_editor) kind: NativeVariadicMergeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct StructuralMergeChild {
    pub(in crate::ui::panels::node_editor) container: NodeContainer,
    pub(in crate::ui::panels::node_editor) owner: PortOwner,
}

pub(in crate::ui::panels::node_editor) fn estimated_merge_input_anchor_offset(
    front_to_back_index: usize,
) -> f32 {
    crate::ui::panels::node_editor::types::MERGE_INPUT_FIRST_ROW_Y
        + front_to_back_index as f32 * crate::ui::panels::node_editor::types::MERGE_INPUT_ROW_STRIDE
}

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
    pub(in crate::ui::panels::node_editor) kind: NativeVariadicMergeKind,
    /// Stable index in the canonical persisted/evaluation order.
    pub(in crate::ui::panels::node_editor) canonical_index: usize,
    /// Physical row index in the Node Editor, always top to bottom.
    pub(in crate::ui::panels::node_editor) visual_index: usize,
    pub(in crate::ui::panels::node_editor) layer_count: usize,
    pub(in crate::ui::panels::node_editor) structural_child: Option<StructuralMergeChild>,
    pub(in crate::ui::panels::node_editor) reorder_min_index: usize,
    pub(in crate::ui::panels::node_editor) reorder_max_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum MergeInputSlotRole {
    Canonical,
    Connected(MergeLayerRow),
    Vacant(NativeVariadicMergeKind),
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
            "ordered_input_node_id": self.merge_id,
            "ordered_input": true,
            "input_kind": self.kind.qa_key(),
            "merge_kind": self.kind.qa_key(),
            "port": self.kind.input_port(),
            "connection_id": self.connection_id,
            "visual_index": self.visual_index,
            "canonical_index": self.canonical_index,
            "layer_count": self.layer_count,
            "reorder_min_canonical_index": self.reorder_min_index,
            "reorder_max_canonical_index": self.reorder_max_index,
            "structural_child": self.structural_child.map(|binding| serde_json::json!({
                "container": format!("{:?}", binding.container),
                "owner": qa_container_key(binding.owner),
                "reorders_timeline": true,
            })),
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
            "canonical_order_semantics": self.kind.canonical_order_semantics(),
            "visual_order_semantics": self.kind.visual_order_semantics(),
            "order_semantics": self.kind.canonical_order_semantics(),
            "blend_ownership": self.kind.has_blend_controls().then_some("connection"),
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
    let Some(merge) = native_variadic_merge_for_node(project, merge_id) else {
        return Vec::new();
    };
    let kind = merge.kind;
    let target = PortAddress::new(PortOwner::Node(merge_id), kind.input_port());
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    connections.sort_by(|left, right| {
        kind.visual_connection_cmp((left.order, left.id), (right.order, right.id))
    });
    let layer_count = connections.len();
    let structural = structural_merge_context(project, merge_id);
    let structural_count = structural
        .as_ref()
        .map_or(0, |(_, children)| children.len())
        .min(layer_count);
    connections
        .into_iter()
        .enumerate()
        .map(|(visual_index, connection)| {
            let canonical_index = kind.canonical_index_for_visual(visual_index, layer_count);
            let structural_child = structural.as_ref().and_then(|(container, children)| {
                let expected_port = match kind {
                    NativeVariadicMergeKind::Image => IMAGE_OUTPUT_PORT,
                    NativeVariadicMergeKind::Sound => AUDIO_OUTPUT_PORT,
                    NativeVariadicMergeKind::List => LIST_ITEM_OUTPUT_PORT,
                };
                (connection.from.port == expected_port && children.contains(&connection.from.owner))
                    .then_some(StructuralMergeChild {
                        container: *container,
                        owner: connection.from.owner,
                    })
            });
            let (reorder_min_index, reorder_max_index) = if structural_child.is_some() {
                (0, structural_count.saturating_sub(1))
            } else {
                (structural_count, layer_count.saturating_sub(1))
            };
            MergeLayerRow {
                merge_id,
                connection_id: connection.id,
                source: connection.from.clone(),
                source_label: merge_layer_source_label(project, connection.from.owner),
                authored_order: connection.order,
                authored_blend_mode: connection.blend_mode,
                authored_blend_available: connection_supports_authored_blend(project, connection),
                kind,
                canonical_index,
                visual_index,
                layer_count,
                structural_child,
                reorder_min_index,
                reorder_max_index,
            }
        })
        .collect()
}

fn structural_merge_context(
    project: &Project,
    merge_id: Uuid,
) -> Option<(NodeContainer, Vec<PortOwner>)> {
    if let Some(composition) = project.compositions.iter().find(|composition| {
        composition.structural_merge_node_id == merge_id
            || composition.structural_sound_merge_node_id == merge_id
    }) {
        return Some((
            NodeContainer::Composition(composition.id),
            composition
                .track_ids
                .iter()
                .copied()
                .map(PortOwner::Track)
                .collect(),
        ));
    }
    project
        .tracks
        .values()
        .find(|track| {
            track.structural_merge_node_id == merge_id
                || track.structural_sound_merge_node_id == merge_id
        })
        .map(|track| {
            (
                NodeContainer::Track(track.id),
                track
                    .clip_ids
                    .iter()
                    .copied()
                    .map(PortOwner::Clip)
                    .collect(),
            )
        })
}

pub(in crate::ui::panels::node_editor) fn merge_vacant_slot(
    project: &Project,
    merge_id: Uuid,
) -> Option<MergeVacantSlot> {
    let merge = native_variadic_merge_for_node(project, merge_id)?;
    let target = PortAddress::new(PortOwner::Node(merge_id), merge.kind.input_port());
    let layer_count = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .count();
    let structural_prefix_len = structural_merge_context(project, merge_id)
        .map_or(0, |(_, children)| children.len())
        .min(layer_count);
    let canonical_index = merge
        .kind
        .vacant_canonical_index(layer_count, structural_prefix_len);
    let visual_index = match merge.kind {
        NativeVariadicMergeKind::Image => layer_count.saturating_sub(canonical_index),
        NativeVariadicMergeKind::Sound | NativeVariadicMergeKind::List => canonical_index,
    };
    Some(MergeVacantSlot {
        canonical_index,
        visual_index,
        layer_count,
        structural_prefix_len,
        insertion_semantics: merge.kind.vacant_insertion_semantics(structural_prefix_len),
    })
}

/// Identify a native typed variadic Merge input. A matching port key alone is
/// intentionally insufficient: plugin operations may declare their own
/// variadic `images` or `sounds` input, and those remain ordinary graph pins.
pub(in crate::ui::panels::node_editor) fn native_variadic_merge_target(
    project: &Project,
    target: &PortAddress,
) -> Option<NativeVariadicMerge> {
    let PortOwner::Node(node_id) = target.owner else {
        return None;
    };
    let merge = native_variadic_merge_for_node(project, node_id)?;
    if target.port != merge.kind.input_port() {
        return None;
    }
    project
        .port_definition(target, PortDirection::Input)
        .is_some_and(|definition| {
            definition.data_type == merge.kind.data_type()
                && definition.multiplicity == PortMultiplicity::Variadic
        })
        .then_some(merge)
}

/// Compare two persisted connections in the exact physical top-to-bottom
/// order used by the native Image/Sound/List variadic rows.
pub(in crate::ui::panels::node_editor) fn native_variadic_connection_visual_cmp(
    project: &Project,
    target: &PortAddress,
    left: (i64, Uuid),
    right: (i64, Uuid),
) -> Option<Ordering> {
    native_variadic_merge_target(project, target)
        .map(|merge| merge.kind.visual_connection_cmp(left, right))
}

pub(in crate::ui::panels::node_editor) fn native_variadic_merge_for_node(
    project: &Project,
    node_id: Uuid,
) -> Option<NativeVariadicMerge> {
    let kind = match project.get_node(node_id).map(|node| node.content()) {
        Some(NodeContent::Merge) => NativeVariadicMergeKind::Image,
        Some(NodeContent::SoundMerge) => NativeVariadicMergeKind::Sound,
        Some(NodeContent::List(ListContent::Make)) => NativeVariadicMergeKind::List,
        _ => return None,
    };
    Some(NativeVariadicMerge { node_id, kind })
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn merge_images_target_node_id(
    project: &Project,
    target: &PortAddress,
) -> Option<Uuid> {
    native_variadic_merge_target(project, target)
        .filter(|target| target.kind == NativeVariadicMergeKind::Image)
        .map(|target| target.node_id)
}

/// Expand a native Merge's typed variadic definition into one physical input
/// pin per canonical connection plus one vacant insertion pin. Image rows are
/// visually front-to-back; Sound rows retain canonical top-to-bottom order.
/// The Project port remains one variadic address.
pub(in crate::ui::panels::node_editor) fn merge_input_slots(
    project: &Project,
    merge_id: Uuid,
) -> Vec<MergeInputSlot> {
    let kind = native_variadic_merge_for_node(project, merge_id).map(|merge| merge.kind);
    let rows = merge_layer_rows(project, merge_id);
    canonical_pin_definitions(
        project,
        PortOwner::Node(merge_id),
        PortDirection::Input,
        library::model::project::PortSide::Left,
    )
    .into_iter()
    .flat_map(|definition| {
        let Some(kind) = kind.filter(|kind| definition.key == kind.input_port()) else {
            return vec![MergeInputSlot {
                definition,
                role: MergeInputSlotRole::Canonical,
            }];
        };
        let mut slots = rows
            .iter()
            .cloned()
            .map(|row| MergeInputSlot {
                definition: definition.clone(),
                role: MergeInputSlotRole::Connected(row),
            })
            .collect::<Vec<_>>();
        let vacant = MergeInputSlot {
            definition,
            role: MergeInputSlotRole::Vacant(kind),
        };
        let vacant_visual_index = merge_vacant_slot(project, merge_id)
            .map_or(slots.len(), |slot| slot.visual_index.min(slots.len()));
        slots.insert(vacant_visual_index, vacant);
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
    let target_is_merge_images = native_variadic_merge_target(project, &connection.to)
        .is_some_and(|target| target.kind == NativeVariadicMergeKind::Image);
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
        assert!(
            linear_dodge
                .keywords
                .iter()
                .any(|keyword| keyword == "linear_dodge")
        );
    }
}
