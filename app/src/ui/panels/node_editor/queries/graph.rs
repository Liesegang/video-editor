use crate::ui::widgets::property_drag_value::FloatDragValueConfig;
use library::model::project::{PortDataType, PortDirection, PortOwner, PortSide};
use library::model::property::PropertyDefinition;
use library::model::{Node, NodeContainer, NodeContent, Project};
use library::plugin::PluginManager;
use uuid::Uuid;

use crate::ui::panels::node_editor::{GraphItem, PinDefinition, PortAnchorKind};

pub(in crate::ui::panels::node_editor) fn node_title(project: &Project, node_id: Uuid) -> String {
    project
        .get_node(node_id)
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "Missing node".to_string())
}

pub(in crate::ui::panels::node_editor) fn graph_item_title(
    project: &Project,
    item: GraphItem,
) -> String {
    match item {
        GraphItem::Node(node_id) => node_title(project, node_id),
        GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
            container_title(project, owner)
        }
    }
}

pub(in crate::ui::panels::node_editor) fn container_title(
    project: &Project,
    owner: PortOwner,
) -> String {
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

pub(in crate::ui::panels::node_editor) fn container_name_and_size(
    project: &Project,
    owner: PortOwner,
) -> Option<(String, [f32; 2])> {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| (composition.name.clone(), composition.ui_size)),
        PortOwner::Track(id) => project
            .get_track(id)
            .map(|track| (track.name.clone(), track.ui_size)),
        PortOwner::Clip(id) => project
            .get_clip(id)
            .map(|clip| (clip.name.clone(), clip.ui_size)),
        PortOwner::Node(_) => None,
    }
}

pub(in crate::ui::panels::node_editor) fn container_collapsed(
    project: &Project,
    owner: PortOwner,
) -> Option<bool> {
    match owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.ui_collapsed),
        PortOwner::Track(id) => project.get_track(id).map(|track| track.ui_collapsed),
        PortOwner::Clip(id) => project.get_clip(id).map(|clip| clip.ui_collapsed),
        PortOwner::Node(_) => None,
    }
}

pub(in crate::ui::panels::node_editor) fn port_owner_for_node_container(
    container: NodeContainer,
) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

pub(in crate::ui::panels::node_editor) fn port_owner_composition(
    project: &Project,
    owner: PortOwner,
) -> Option<Uuid> {
    match owner {
        PortOwner::Composition(composition_id) => project
            .get_composition(composition_id)
            .map(|_| composition_id),
        PortOwner::Track(track_id) => project.find_composition_for_track(track_id),
        PortOwner::Clip(clip_id) => project
            .find_track_for_clip(clip_id)
            .and_then(|track_id| project.find_composition_for_track(track_id)),
        PortOwner::Node(node_id) => project.find_node_container(node_id).and_then(|container| {
            port_owner_composition(project, port_owner_for_node_container(container))
        }),
    }
}

pub(in crate::ui::panels::node_editor) fn parent_container_owner(
    project: &Project,
    owner: PortOwner,
) -> Option<PortOwner> {
    match owner {
        PortOwner::Composition(_) | PortOwner::Node(_) => None,
        PortOwner::Track(track_id) => project
            .find_composition_for_track(track_id)
            .map(PortOwner::Composition),
        PortOwner::Clip(clip_id) => project.find_track_for_clip(clip_id).map(PortOwner::Track),
    }
}
/// Node properties share the evaluator's time domain. A Node directly owned
/// by a Clip is evaluated and edited in that Clip's source-local time; Nodes
/// owned directly by a Track or Composition stay in global composition time.
pub(in crate::ui::panels::node_editor) fn node_property_time(
    project: &Project,
    node_id: Uuid,
    global_time: f64,
) -> f64 {
    project
        .find_parent_clip(node_id)
        .and_then(|clip_id| project.get_clip(clip_id))
        .map_or(global_time, |clip| clip.local_time(global_time))
}

pub(in crate::ui::panels::node_editor) fn node_property_definition(
    plugin_manager: Option<&PluginManager>,
    node: &Node,
    property_name: &str,
) -> Option<PropertyDefinition> {
    match node.content() {
        NodeContent::Value(value) => value
            .property_definitions()
            .iter()
            .find(|definition| definition.name() == property_name)
            .cloned(),
        NodeContent::SoundAnalysis(analysis) => analysis
            .property_definitions()
            .iter()
            .find(|definition| definition.name() == property_name)
            .cloned(),
        NodeContent::PluginOperation(operation) => plugin_manager?
            .operation_descriptor(
                &operation.category,
                &operation.component_id,
                &operation.operation,
            )
            .ok()?
            .properties()
            .iter()
            .find(|definition| definition.name() == property_name)
            .cloned(),
        _ => None,
    }
}

pub(in crate::ui::panels) fn node_timing_drag_config(
    definition: &library::model::property::PropertyDefinition,
) -> Option<FloatDragValueConfig> {
    FloatDragValueConfig::from_definition(definition)
}

pub(in crate::ui::panels::node_editor) fn clip_is_active(
    clip: &library::model::Clip,
    current_time: f64,
) -> bool {
    current_time >= clip.start_time.into_inner() && current_time < clip.end_time()
}

pub(in crate::ui::panels::node_editor) fn container_inactive(
    project: &Project,
    owner: PortOwner,
    current_time: f64,
) -> bool {
    match owner {
        PortOwner::Clip(id) => project
            .get_clip(id)
            .is_some_and(|clip| !clip_is_active(clip, current_time)),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum GraphItemInactiveReason {
    Disabled,
    OutsideClipRange,
}

impl GraphItemInactiveReason {
    pub(in crate::ui::panels::node_editor) fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::OutsideClipRange => "outside_clip_range",
        }
    }

    pub(in crate::ui::panels::node_editor) fn tooltip(self) -> &'static str {
        match self {
            Self::Disabled => "No output (Node disabled)",
            Self::OutsideClipRange => "No output (outside Clip range)",
        }
    }
}

pub(in crate::ui::panels::node_editor) fn graph_item_inactive_reason(
    project: &Project,
    item: GraphItem,
    current_time: f64,
) -> Option<GraphItemInactiveReason> {
    match item {
        GraphItem::Node(node_id) => {
            if project.get_node(node_id).is_some_and(|node| !node.enabled) {
                return Some(GraphItemInactiveReason::Disabled);
            }
            project
                .find_parent_clip(node_id)
                .and_then(|clip_id| project.get_clip(clip_id))
                .filter(|clip| !clip_is_active(clip, current_time))
                .map(|_| GraphItemInactiveReason::OutsideClipRange)
        }
        GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
            container_inactive(project, owner, current_time)
                .then_some(GraphItemInactiveReason::OutsideClipRange)
        }
    }
}

pub(in crate::ui::panels::node_editor) fn graph_item_inactive(
    project: &Project,
    item: GraphItem,
    current_time: f64,
) -> bool {
    graph_item_inactive_reason(project, item, current_time).is_some()
}

pub(in crate::ui::panels::node_editor) fn input_definitions(
    project: &Project,
    item: GraphItem,
) -> Vec<PinDefinition> {
    let owner = match item {
        GraphItem::Node(node_id) => PortOwner::Node(node_id),
        GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::ExternalInputs,
        } => owner,
        GraphItem::PortAnchor {
            kind: PortAnchorKind::OutputSinks,
            ..
        } => {
            return vec![
                PinDefinition {
                    key: crate::ui::panels::node_editor::IMAGE_OUTPUT_BINDING_PORT.to_string(),
                    name: "Image".to_string(),
                    data_type: PortDataType::Image,
                },
                PinDefinition {
                    key: crate::ui::panels::node_editor::AUDIO_OUTPUT_BINDING_PORT.to_string(),
                    name: "Audio".to_string(),
                    data_type: PortDataType::Audio,
                },
            ];
        }
        GraphItem::Container(_) | GraphItem::PortAnchor { .. } => return Vec::new(),
    };
    canonical_pin_definitions(project, owner, PortDirection::Input, PortSide::Left)
}

pub(in crate::ui::panels::node_editor) fn output_definitions(
    project: &Project,
    item: GraphItem,
) -> Vec<PinDefinition> {
    let (owner, side) = match item {
        GraphItem::Node(node_id) => (PortOwner::Node(node_id), PortSide::Right),
        GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::InternalMetadata,
        } => (owner, PortSide::Left),
        GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::ExternalOutputs,
        } => (owner, PortSide::Right),
        GraphItem::Container(_) | GraphItem::PortAnchor { .. } => return Vec::new(),
    };
    canonical_pin_definitions(project, owner, PortDirection::Output, side)
}

pub(in crate::ui::panels::node_editor) fn canonical_pin_definitions(
    project: &Project,
    owner: PortOwner,
    direction: PortDirection,
    side: PortSide,
) -> Vec<PinDefinition> {
    project
        .port_definitions(owner)
        .into_iter()
        .filter(|definition| definition.direction == direction && definition.side == side)
        .map(|definition| PinDefinition {
            key: definition.key,
            name: definition.label,
            data_type: definition.data_type,
        })
        .collect()
}
