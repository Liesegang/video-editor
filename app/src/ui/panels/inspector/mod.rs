use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use egui::Ui;
use library::model::node::{
    CLIP_DURATION_PROPERTY, CLIP_START_TIME_PROPERTY, CLIP_TIME_STRETCH_PROPERTY,
    CLIP_TRIM_IN_PROPERTY,
};
use library::model::project::{
    ContainerGraphSemantics, PortOwner, Project, ProjectConnection, IMAGE_INPUT_PORT,
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue};
use library::model::{Clip, Composition, GeneratorContent, Node, NodeContent, Track};
use library::plugin::{PluginManager, TRANSFORM_CATEGORY};
use library::{EditorService, PropertyOwner};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::ui::widgets::property_drag_value::FloatDragValueConfig;
use crate::{
    action::HistoryManager,
    state::{context::EditorContext, context_types::SelectionTarget},
};

pub mod action_handler;
mod evaluation;
mod path_effect;
mod presentation;
pub mod properties;
mod property_authoring;
mod property_inference;

use action_handler::ActionContext;
use evaluation::{evaluate_property_map, render_evaluation_issues};
use presentation::{
    render_multi_selection_notice, render_node_time_source, resolve_node_time_source,
    NodeTimeSource,
};
use properties::{render_property_rows, PropertyRenderContext};
use property_inference::inferred_property_definitions;
#[cfg(test)]
use property_inference::property_label;

#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "the Inspector takes one short-lived authoritative selection snapshot per frame"
)]
enum InspectorSelection {
    Composition {
        composition: Composition,
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        semantics: ContainerGraphSemantics,
    },
    Track {
        track: Track,
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        semantics: ContainerGraphSemantics,
    },
    Clip {
        clip: Clip,
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        semantics: ContainerGraphSemantics,
        track_id: Option<Uuid>,
    },
    Node {
        node: Node,
        track_id: Option<Uuid>,
        containing_clip: Option<Clip>,
        time_source: Option<NodeTimeSource>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FacadeOwnerKind {
    Composition,
    Track,
    Clip,
}

const OPERATION_CATEGORY_SECTIONS: [(&str, &str, &str); 6] = [
    (TRANSFORM_CATEGORY, "Transform", "Root placement"),
    path_effect::CATEGORY_SECTION,
    ("decorator", "Decorator", "Shape modifier"),
    ("effector", "Effector", "Shape modifier"),
    ("style", "Style", "Appearance"),
    ("effect", "Effect", "Image effect"),
];

impl FacadeOwnerKind {
    fn qa_value(self) -> &'static str {
        match self {
            Self::Composition => "composition",
            Self::Track => "track",
            Self::Clip => "clip",
        }
    }

    fn output_mode(self, output_node_id: Option<Uuid>) -> FacadeOutputMode {
        match self {
            Self::Composition | Self::Track => FacadeOutputMode::TimelineChildren(output_node_id),
            Self::Clip => {
                output_node_id.map_or(FacadeOutputMode::NoOutput, FacadeOutputMode::Explicit)
            }
        }
    }

    fn derived_children_label(self) -> Option<&'static str> {
        match self {
            Self::Composition => Some("ordered child Tracks"),
            Self::Track => Some("ordered child Clips"),
            Self::Clip => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FacadeOutputMode {
    Explicit(Uuid),
    TimelineChildren(Option<Uuid>),
    NoOutput,
}

impl FacadeOutputMode {
    fn qa_value(self) -> &'static str {
        match self {
            Self::Explicit(_) => "explicit",
            Self::TimelineChildren(_) => "timeline_children",
            Self::NoOutput => "no_output",
        }
    }

    fn explicit_node_id(self) -> Option<Uuid> {
        match self {
            Self::Explicit(node_id) => Some(node_id),
            Self::TimelineChildren(node_id) => node_id,
            Self::NoOutput => None,
        }
    }
}

pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    crate::qa::register_component(
        "inspector.scroll_area",
        "inspector_scroll_area",
        ui.available_rect_before_wrap(),
    );
    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .show(ui, |ui| {
            inspector_panel_content(
                ui,
                editor_context,
                history_manager,
                project_service,
                project,
            );
        });
}

fn inspector_panel_content(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    let Some(composition_id) = editor_context.active_composition_id else {
        ui.label("No composition selected.");
        return;
    };
    let selection = match project.read() {
        Ok(project) => {
            resolve_selection(&project, editor_context.selection.primary(), composition_id)
        }
        Err(error) => {
            log::error!("Failed to read Project for Inspector: {error}");
            ui.label("Project is temporarily unavailable.");
            return;
        }
    };

    let Some(selection) = selection else {
        ui.label("The selected Timeline item was not found (it may have been deleted).");
        editor_context.clear_selection();
        return;
    };

    let (fps, resolution) = project_service
        .get_composition(composition_id)
        .map(|composition| (composition.fps, (composition.width, composition.height)))
        .unwrap_or((60.0, (1920, 1080)));
    let global_time = editor_context.timeline.current_time as f64;
    let mut needs_refresh = false;

    render_multi_selection_notice(ui, editor_context);

    match selection {
        InspectorSelection::Composition {
            composition,
            nodes,
            connections,
            semantics,
        } => {
            let heading = ui.heading(format!("Composition: {}", composition.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.composition:{}", composition.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({"owner": "composition", "id": composition.id})),
            );
            ui.separator();
            render_semantic_graph_facade(
                ui,
                "Composition Output",
                FacadeOwnerKind::Composition,
                &semantics,
                &nodes,
                &connections,
                composition_id,
                None,
                global_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
        InspectorSelection::Track {
            track,
            nodes,
            connections,
            semantics,
        } => {
            let heading = ui.heading(format!("Track: {}", track.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.track:{}", track.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({"owner": "track", "id": track.id})),
            );
            ui.separator();
            render_semantic_graph_facade(
                ui,
                "Track Output",
                FacadeOwnerKind::Track,
                &semantics,
                &nodes,
                &connections,
                composition_id,
                Some(track.id),
                global_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
        InspectorSelection::Clip {
            clip,
            nodes,
            connections,
            semantics,
            track_id,
        } => {
            let heading = ui.heading(format!("Clip: {}", clip.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.clip:{}", clip.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({
                    "owner": "clip",
                    "id": clip.id,
                    "track_id": track_id,
                })),
            );
            ui.separator();

            render_clip_timing(
                ui,
                &clip,
                fps,
                project_service,
                history_manager,
                project,
                &mut needs_refresh,
            );

            let local_time = clip.local_time(global_time);
            let mut clip_definitions = inferred_property_definitions(&clip.properties, local_time);
            clip_definitions.retain(|definition| !is_clip_timing_property(definition.name()));
            if !clip_definitions.is_empty() {
                ui.add_space(10.0);
                ui.heading("Clip Properties");
                ui.separator();
                render_property_map(
                    ui,
                    project_service,
                    history_manager,
                    editor_context,
                    PropertyOwner::Clip(clip.id),
                    &clip.properties,
                    clip_definitions,
                    local_time,
                    fps,
                    resolution,
                    &mut needs_refresh,
                );
            }

            ui.add_space(12.0);
            render_semantic_graph_facade(
                ui,
                "Clip Output",
                FacadeOwnerKind::Clip,
                &semantics,
                &nodes,
                &connections,
                composition_id,
                track_id,
                local_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
        InspectorSelection::Node {
            node,
            track_id,
            containing_clip,
            time_source,
        } => {
            let heading = ui.heading(format!("Node: {}", node.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.node:{}", node.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({"owner": "node", "id": node.id})),
            );
            ui.separator();
            if let Some(time_source) = time_source.as_ref() {
                render_node_time_source(ui, node.id, time_source);
            }
            let evaluation_time = containing_clip
                .as_ref()
                .map_or(global_time, |clip| clip.local_time(global_time));
            render_node(
                ui,
                &node,
                composition_id,
                track_id,
                evaluation_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}

fn resolve_selection(
    project: &Project,
    selected_target: Option<SelectionTarget>,
    composition_id: Uuid,
) -> Option<InspectorSelection> {
    match selected_target {
        Some(SelectionTarget::Clip(clip_id))
            if project
                .find_track_for_clip(clip_id)
                .and_then(|track_id| project.find_composition_for_track(track_id))
                == Some(composition_id) =>
        {
            if let Some(clip) = project.get_clip(clip_id) {
                let nodes = nodes_for_ids(project, &clip.node_ids);
                return Some(InspectorSelection::Clip {
                    clip: clip.clone(),
                    connections: connections_for_nodes(project, &clip.node_ids),
                    nodes,
                    semantics: project.container_graph_semantics(PortOwner::Clip(clip.id)),
                    track_id: project.find_track_for_clip(clip_id),
                });
            }
        }
        Some(SelectionTarget::Node(node_id))
            if node_containing_composition(project, node_id) == Some(composition_id) =>
        {
            if let Some(node) = project.get_node(node_id) {
                let containing_clip = project
                    .find_parent_clip(node_id)
                    .and_then(|clip_id| project.get_clip(clip_id))
                    .cloned();
                return Some(InspectorSelection::Node {
                    node: node.clone(),
                    track_id: node_containing_track(project, node_id),
                    containing_clip,
                    time_source: resolve_node_time_source(project, node_id),
                });
            }
        }
        Some(SelectionTarget::Track(track_id))
            if project.find_composition_for_track(track_id) == Some(composition_id) =>
        {
            if let Some(track) = project.get_track(track_id) {
                return Some(InspectorSelection::Track {
                    track: track.clone(),
                    nodes: nodes_for_ids(project, &track.node_ids),
                    connections: connections_for_nodes(project, &track.node_ids),
                    semantics: project.container_graph_semantics(PortOwner::Track(track.id)),
                });
            }
        }
        Some(SelectionTarget::Composition(selected_composition_id))
            if selected_composition_id == composition_id => {}
        Some(
            SelectionTarget::Node(_)
            | SelectionTarget::Clip(_)
            | SelectionTarget::Track(_)
            | SelectionTarget::Composition(_),
        ) => return None,
        None => {}
    }

    let composition = project.get_composition(composition_id)?;
    Some(InspectorSelection::Composition {
        composition: composition.clone(),
        nodes: nodes_for_ids(project, &composition.node_ids),
        connections: connections_for_nodes(project, &composition.node_ids),
        semantics: project.container_graph_semantics(PortOwner::Composition(composition.id)),
    })
}

fn node_containing_composition(project: &Project, node_id: Uuid) -> Option<Uuid> {
    match project.find_node_container(node_id)? {
        library::model::NodeContainer::Composition(id) => Some(id),
        library::model::NodeContainer::Track(id) => project.find_composition_for_track(id),
        library::model::NodeContainer::Clip(id) => project
            .find_track_for_clip(id)
            .and_then(|track_id| project.find_composition_for_track(track_id)),
    }
}

fn node_containing_track(project: &Project, node_id: Uuid) -> Option<Uuid> {
    match project.find_node_container(node_id)? {
        library::model::NodeContainer::Composition(_) => None,
        library::model::NodeContainer::Track(track_id) => Some(track_id),
        library::model::NodeContainer::Clip(clip_id) => project.find_track_for_clip(clip_id),
    }
}

fn nodes_for_ids(project: &Project, node_ids: &[Uuid]) -> Vec<Node> {
    node_ids
        .iter()
        .filter_map(|node_id| project.get_node(*node_id).cloned())
        .collect()
}

fn connections_for_nodes(project: &Project, node_ids: &[Uuid]) -> Vec<ProjectConnection> {
    let node_ids = node_ids.iter().copied().collect::<HashSet<_>>();
    project
        .connections
        .iter()
        .filter(|connection| {
            matches!(connection.from.owner, PortOwner::Node(id) if node_ids.contains(&id))
                || matches!(connection.to.owner, PortOwner::Node(id) if node_ids.contains(&id))
        })
        .cloned()
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "the Timeline facade renders one authoritative graph snapshot with editing, timing, history, and QA context"
)]
fn render_semantic_graph_facade(
    ui: &mut Ui,
    output_label: &str,
    owner_kind: FacadeOwnerKind,
    semantics: &ContainerGraphSemantics,
    nodes: &[Node],
    connections: &[ProjectConnection],
    composition_id: Uuid,
    track_id: Option<Uuid>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    let output_node_id = semantics.explicit_output_node_id();
    let output_mode = owner_kind.output_mode(output_node_id);
    let sources = semantic_visual_sources(nodes);
    let values = native_value_nodes(nodes);
    let merges = nodes
        .iter()
        .filter(|node| matches!(node.content(), NodeContent::Merge))
        .collect::<Vec<_>>();
    let operations = nodes
        .iter()
        .filter(|node| matches!(node.content(), NodeContent::PluginOperation(_)))
        .collect::<Vec<_>>();

    ui.heading("Source");
    ui.separator();
    if sources.is_empty() {
        ui.label("Source comes from connected Timeline content.");
    }
    for source in sources {
        let is_result = output_mode.explicit_node_id() == Some(source.id);
        let wired_to_result = semantics.structurally_reaches_output(PortOwner::Node(source.id));
        let outgoing = connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Node(source.id)
                    && is_content_flow_connection(connection)
            })
            .collect::<Vec<_>>();
        let title = if !source.enabled {
            format!("{} · Disabled", source_semantic_label(source))
        } else if source.bypassed {
            format!("{} · Bypassed", source_semantic_label(source))
        } else if is_result {
            format!("{} · Result", source_semantic_label(source))
        } else if wired_to_result {
            format!("{} · Wired to result", source_semantic_label(source))
        } else {
            format!("{} · Not wired to result", source_semantic_label(source))
        };
        let response = egui::CollapsingHeader::new(title)
            .id_salt(("inspector_source", source.id))
            .default_open(is_result || nodes.len() == 1)
            .show(ui, |ui| {
                render_node_properties(
                    ui,
                    source,
                    composition_id,
                    track_id,
                    current_time,
                    fps,
                    resolution,
                    project_service,
                    history_manager,
                    editor_context,
                    needs_refresh,
                );
            });
        let connection_metadata = outgoing
            .iter()
            .map(|connection| content_connection_metadata(connection))
            .collect::<Vec<_>>();
        crate::qa::register_component_with_metadata(
            format!("inspector.source:{}", source.id),
            "inspector_source_item",
            response.header_response.rect,
            true,
            Some(serde_json::json!({
                "source_id": source.id,
                "source_kind": source_kind(source),
                "is_result": is_result,
                "structurally_reaches_result": wired_to_result,
                "enabled": source.enabled,
                "bypassed": source.bypassed,
                "connection_count": outgoing.len(),
                "connections": connection_metadata,
            })),
        );
    }

    render_value_category(
        ui,
        &values,
        nodes,
        connections,
        composition_id,
        track_id,
        current_time,
        fps,
        resolution,
        project_service,
        history_manager,
        editor_context,
        needs_refresh,
    );

    let mut rendered_categories = HashSet::new();
    for (category, title, meaning) in OPERATION_CATEGORY_SECTIONS {
        let matching = operations
            .iter()
            .copied()
            .filter(|node| operation_category(node) == Some(category))
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        rendered_categories.insert(category.to_string());
        render_operation_category(
            ui,
            title,
            meaning,
            &matching,
            nodes,
            connections,
            semantics,
            composition_id,
            track_id,
            current_time,
            fps,
            resolution,
            project_service,
            history_manager,
            editor_context,
            needs_refresh,
        );
    }

    let mut other_categories = operations
        .iter()
        .filter_map(|node| operation_category(node))
        .filter(|category| !rendered_categories.contains(*category))
        .map(str::to_string)
        .collect::<Vec<_>>();
    other_categories.sort();
    other_categories.dedup();
    for category in other_categories {
        let matching = operations
            .iter()
            .copied()
            .filter(|node| operation_category(node) == Some(category.as_str()))
            .collect::<Vec<_>>();
        render_operation_category(
            ui,
            &category,
            "Plug-in",
            &matching,
            nodes,
            connections,
            semantics,
            composition_id,
            track_id,
            current_time,
            fps,
            resolution,
            project_service,
            history_manager,
            editor_context,
            needs_refresh,
        );
    }

    render_merge_category(
        ui,
        &merges,
        nodes,
        connections,
        semantics,
        composition_id,
        track_id,
        current_time,
        fps,
        resolution,
        project_service,
        history_manager,
        editor_context,
        needs_refresh,
    );

    ui.add_space(10.0);
    ui.heading(output_label);
    ui.separator();
    let output_text = facade_output_text(owner_kind, output_mode, nodes);
    let output_response = ui.label(output_text);
    crate::qa::register_component_with_metadata(
        format!("inspector.output:{}", output_node_id.unwrap_or_default()),
        "inspector_output",
        output_response.rect,
        true,
        Some(facade_output_metadata(
            owner_kind,
            output_mode,
            semantics.explicit_output_is_directly_contained(),
        )),
    );

    if operations.is_empty() && merges.is_empty() {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("No explicit appearance, animation, or compositing Nodes.").weak(),
        );
    }
}

fn semantic_visual_sources(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|node| node.content().is_semantic_visual_source())
        .collect()
}

fn native_value_nodes(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|node| matches!(node.content(), NodeContent::Value(_)))
        .collect()
}

fn value_connection_source_label(connection: &ProjectConnection, nodes: &[Node]) -> String {
    let owner = match connection.from.owner {
        PortOwner::Node(id) => nodes
            .iter()
            .find(|node| node.id == id)
            .map_or_else(|| "Node".to_string(), source_semantic_label),
        PortOwner::Composition(_) => "Composition".to_string(),
        PortOwner::Track(_) => "Track".to_string(),
        PortOwner::Clip(_) => "Clip".to_string(),
    };
    format!("{owner}.{}", connection.from.port)
}

#[allow(
    clippy::too_many_arguments,
    reason = "native Value presentation shares the authoritative Inspector editing context"
)]
fn render_value_category(
    ui: &mut Ui,
    values: &[&Node],
    all_nodes: &[Node],
    connections: &[ProjectConnection],
    composition_id: Uuid,
    track_id: Option<Uuid>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    if values.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.heading("Math / Values");
        ui.label(egui::RichText::new("Explicit numeric graph").small().weak());
    });
    ui.separator();

    for node in values {
        let value = match node.content() {
            NodeContent::Value(value) => *value,
            _ => continue,
        };
        let incoming = connections
            .iter()
            .filter(|connection| connection.to.owner == PortOwner::Node(node.id))
            .collect::<Vec<_>>();
        let outgoing = connections
            .iter()
            .filter(|connection| connection.from.owner == PortOwner::Node(node.id))
            .collect::<Vec<_>>();
        let state = if !node.enabled {
            "Disabled".to_string()
        } else if node.bypassed {
            "Bypassed".to_string()
        } else if outgoing.is_empty() {
            "Not wired".to_string()
        } else if outgoing.len() == 1 {
            "Wired".to_string()
        } else {
            format!("Wired to {} inputs", outgoing.len())
        };
        let response = egui::CollapsingHeader::new(format!("{} · {state}", value.label()))
            .id_salt(("inspector_value", node.id))
            .default_open(outgoing.len() == 1)
            .show(ui, |ui| {
                for connection in &incoming {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} ← {}",
                            connection.to.port,
                            value_connection_source_label(connection, all_nodes),
                        ))
                        .small()
                        .weak(),
                    );
                }
                for connection in &outgoing {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} → {}.{}",
                            connection.from.port,
                            connection_target_label(connection, all_nodes),
                            connection.to.port,
                        ))
                        .small()
                        .weak(),
                    );
                }
                render_node_properties(
                    ui,
                    node,
                    composition_id,
                    track_id,
                    current_time,
                    fps,
                    resolution,
                    project_service,
                    history_manager,
                    editor_context,
                    needs_refresh,
                );
            });
        crate::qa::register_component_with_metadata(
            format!("inspector.value:{}", node.id),
            "inspector_value_item",
            response.header_response.rect,
            true,
            Some(serde_json::json!({
                "value_id": node.id,
                "category": "math_values",
                "operation": value.operation_key(),
                "enabled": node.enabled,
                "bypassed": node.bypassed,
                "input_connection_count": incoming.len(),
                "output_connection_count": outgoing.len(),
                "inputs": incoming.iter().map(|connection| content_connection_metadata(connection)).collect::<Vec<_>>(),
                "outputs": outgoing.iter().map(|connection| content_connection_metadata(connection)).collect::<Vec<_>>(),
            })),
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "an operation category preserves graph connection metadata while sharing Inspector editing context"
)]
fn render_operation_category(
    ui: &mut Ui,
    title: &str,
    meaning: &str,
    operations: &[&Node],
    all_nodes: &[Node],
    connections: &[ProjectConnection],
    semantics: &ContainerGraphSemantics,
    composition_id: Uuid,
    track_id: Option<Uuid>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.label(egui::RichText::new(meaning).small().weak());
    });
    ui.separator();

    for node in operations {
        let NodeContent::PluginOperation(operation) = node.content() else {
            continue;
        };
        let descriptor = project_service.get_plugin_manager().operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        );
        let available = descriptor.is_ok();
        let label = descriptor.as_ref().map_or_else(
            |_| operation.component_id.clone(),
            |value| value.label().to_string(),
        );
        let outgoing = connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Node(node.id)
                    && is_content_flow_connection(connection)
            })
            .collect::<Vec<_>>();
        let is_result = semantics.explicit_output_node_id() == Some(node.id);
        let wired_to_result = semantics.structurally_reaches_output(PortOwner::Node(node.id));
        let state = operation_state_label(
            available,
            node.enabled,
            node.bypassed,
            is_result,
            wired_to_result,
            outgoing.as_slice(),
        );
        let response = egui::CollapsingHeader::new(format!("{label} · {state}"))
            .id_salt(("inspector_operation", node.id))
            .default_open(is_result || outgoing.len() == 1)
            .show(ui, |ui| {
                if !available {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "Plug-in unavailable; authored settings are preserved.",
                    );
                } else if !is_result && outgoing.is_empty() {
                    ui.label(egui::RichText::new("Not connected to this result.").weak());
                }
                for connection in &outgoing {
                    ui.label(
                        egui::RichText::new(format!(
                            "Feeds {} · order {}",
                            connection_target_label(connection, all_nodes),
                            connection.order
                        ))
                        .small()
                        .weak(),
                    );
                }
                render_node_properties(
                    ui,
                    node,
                    composition_id,
                    track_id,
                    current_time,
                    fps,
                    resolution,
                    project_service,
                    history_manager,
                    editor_context,
                    needs_refresh,
                );
            });
        let connection_metadata = outgoing
            .iter()
            .map(|connection| content_connection_metadata(connection))
            .collect::<Vec<_>>();
        crate::qa::register_component_with_metadata(
            format!("inspector.operation:{}", node.id),
            "inspector_operation_item",
            response.header_response.rect,
            true,
            Some(serde_json::json!({
                "operation_id": node.id,
                "category": operation.category,
                "component_id": operation.component_id,
                "operation": operation.operation,
                "available": available,
                "enabled": node.enabled,
                "bypassed": node.bypassed,
                "is_result": is_result,
                "structurally_reaches_result": wired_to_result,
                "connection_count": outgoing.len(),
                "connections": connection_metadata,
                "shape_geometry": path_effect::is_category(&operation.category).then_some(path_effect::SUPPORTED_GEOMETRY),
                "unsupported_shape_geometry": path_effect::is_category(&operation.category).then_some(path_effect::UNSUPPORTED_GEOMETRY),
            })),
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Merge presentation preserves ordered graph inputs while sharing the Inspector editing context"
)]
fn render_merge_category(
    ui: &mut Ui,
    merges: &[&Node],
    all_nodes: &[Node],
    connections: &[ProjectConnection],
    semantics: &ContainerGraphSemantics,
    composition_id: Uuid,
    track_id: Option<Uuid>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    if merges.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.heading("Compositing");
        ui.label(egui::RichText::new("Merge").small().weak());
    });
    ui.separator();

    for merge in merges {
        let incoming = connections
            .iter()
            .filter(|connection| {
                connection.to.owner == PortOwner::Node(merge.id)
                    && is_content_flow_connection(connection)
            })
            .collect::<Vec<_>>();
        let outgoing = connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Node(merge.id)
                    && is_content_flow_connection(connection)
            })
            .collect::<Vec<_>>();
        let is_result = semantics.explicit_output_node_id() == Some(merge.id);
        let wired_to_result = semantics.structurally_reaches_output(PortOwner::Node(merge.id));
        let state = operation_state_label(
            true,
            merge.enabled,
            merge.bypassed,
            is_result,
            wired_to_result,
            outgoing.as_slice(),
        );
        let response = egui::CollapsingHeader::new(format!("Merge · {state}"))
            .id_salt(("inspector_merge", merge.id))
            .default_open(is_result)
            .show(ui, |ui| {
                if incoming.is_empty() {
                    ui.label(egui::RichText::new("No image inputs connected.").weak());
                }
                for connection in &incoming {
                    ui.label(
                        egui::RichText::new(format!(
                            "Input: {} · order {}",
                            connection_source_label(connection, all_nodes),
                            connection.order
                        ))
                        .small()
                        .weak(),
                    );
                }
                for connection in &outgoing {
                    ui.label(
                        egui::RichText::new(format!(
                            "Feeds {} · order {}",
                            connection_target_label(connection, all_nodes),
                            connection.order
                        ))
                        .small()
                        .weak(),
                    );
                }
                render_node_properties(
                    ui,
                    merge,
                    composition_id,
                    track_id,
                    current_time,
                    fps,
                    resolution,
                    project_service,
                    history_manager,
                    editor_context,
                    needs_refresh,
                );
            });
        let incoming_metadata = incoming
            .iter()
            .map(|connection| {
                serde_json::json!({
                    "connection_id": connection.id,
                    "from_owner": connection.from.owner,
                    "from_port": connection.from.port,
                    "order": connection.order,
                })
            })
            .collect::<Vec<_>>();
        let outgoing_metadata = outgoing
            .iter()
            .map(|connection| {
                serde_json::json!({
                    "connection_id": connection.id,
                    "to_owner": connection.to.owner,
                    "to_port": connection.to.port,
                    "order": connection.order,
                })
            })
            .collect::<Vec<_>>();
        crate::qa::register_component_with_metadata(
            format!("inspector.merge:{}", merge.id),
            "inspector_compositing_item",
            response.header_response.rect,
            true,
            Some(serde_json::json!({
                "operation_id": merge.id,
                "category": "merge",
                "available": true,
                "enabled": merge.enabled,
                "bypassed": merge.bypassed,
                "is_result": is_result,
                "structurally_reaches_result": wired_to_result,
                "inputs": incoming_metadata,
                "outputs": outgoing_metadata,
            })),
        );
    }
}

fn facade_output_text(
    owner_kind: FacadeOwnerKind,
    output_mode: FacadeOutputMode,
    nodes: &[Node],
) -> String {
    match output_mode {
        FacadeOutputMode::Explicit(node_id) => {
            nodes.iter().find(|node| node.id == node_id).map_or_else(
                || "Explicit Result node is unavailable".to_string(),
                |node| format!("Result: {}", source_semantic_label(node)),
            )
        }
        FacadeOutputMode::TimelineChildren(output_node_id) => format!(
            "Composes {} through the structural Merge{}",
            owner_kind
                .derived_children_label()
                .unwrap_or("ordered child containers"),
            if output_node_id.is_some() {
                " and authored downstream graph"
            } else {
                " (NoOutput: no result binding)"
            }
        ),
        FacadeOutputMode::NoOutput => "No output selected (NoOutput)".to_string(),
    }
}

fn facade_output_metadata(
    owner_kind: FacadeOwnerKind,
    output_mode: FacadeOutputMode,
    explicit_output_is_directly_contained: bool,
) -> serde_json::Value {
    serde_json::json!({
        "output_node_id": output_mode.explicit_node_id(),
        "explicit": output_mode.explicit_node_id().is_some(),
        "explicit_output_is_directly_contained": explicit_output_is_directly_contained,
        "owner_kind": owner_kind.qa_value(),
        "output_mode": output_mode.qa_value(),
    })
}

fn is_content_flow_connection(connection: &ProjectConnection) -> bool {
    matches!(
        (connection.from.port.as_str(), connection.to.port.as_str()),
        (IMAGE_OUTPUT_PORT, IMAGE_INPUT_PORT | MERGE_IMAGES_PORT)
            | (SHAPE_OUTPUT_PORT, SHAPE_INPUT_PORT)
    )
}

fn content_connection_metadata(connection: &ProjectConnection) -> serde_json::Value {
    serde_json::json!({
        "connection_id": connection.id,
        "from_owner": connection.from.owner,
        "from_port": connection.from.port,
        "to_owner": connection.to.owner,
        "to_port": connection.to.port,
        "order": connection.order,
    })
}

fn operation_category(node: &Node) -> Option<&str> {
    let NodeContent::PluginOperation(operation) = node.content() else {
        return None;
    };
    Some(&operation.category)
}

fn operation_state_label(
    available: bool,
    enabled: bool,
    bypassed: bool,
    is_result: bool,
    wired_to_result: bool,
    outgoing: &[&ProjectConnection],
) -> String {
    if !enabled {
        return "Disabled".to_string();
    }
    if bypassed {
        return "Bypassed".to_string();
    }
    if !available {
        return "Unavailable".to_string();
    }
    if is_result {
        return "Result".to_string();
    }
    if !wired_to_result {
        return "Not wired to result".to_string();
    }
    match outgoing {
        [] => "Wired to result".to_string(),
        [connection] => format!("Wired to result · order {}", connection.order),
        _ => format!("Wired to result on {} branches", outgoing.len()),
    }
}

fn connection_target_label(connection: &ProjectConnection, nodes: &[Node]) -> String {
    match connection.to.owner {
        PortOwner::Node(id) => nodes
            .iter()
            .find(|node| node.id == id)
            .map_or_else(|| "connected source".to_string(), source_semantic_label),
        PortOwner::Composition(_) => "composition result".to_string(),
        PortOwner::Track(_) => "track result".to_string(),
        PortOwner::Clip(_) => "clip result".to_string(),
    }
}

fn connection_source_label(connection: &ProjectConnection, nodes: &[Node]) -> String {
    match connection.from.owner {
        PortOwner::Node(id) => nodes
            .iter()
            .find(|node| node.id == id)
            .map_or_else(|| "connected image".to_string(), source_semantic_label),
        PortOwner::Composition(_) => "composition image".to_string(),
        PortOwner::Track(_) => "track image".to_string(),
        PortOwner::Clip(_) => "clip image".to_string(),
    }
}

fn source_semantic_label(node: &Node) -> String {
    let kind = source_kind(node);
    if node.name.eq_ignore_ascii_case(kind) {
        kind.to_string()
    } else {
        format!("{kind} · {}", node.name)
    }
}

fn source_kind(node: &Node) -> &'static str {
    match node.content() {
        NodeContent::Media(_) => "Media",
        NodeContent::Generator(GeneratorContent::Text) => "Text",
        NodeContent::Generator(GeneratorContent::Shape) => "Shape",
        NodeContent::Generator(GeneratorContent::Solid) => "Solid",
        NodeContent::Generator(GeneratorContent::SkSL) => "Shader",
        NodeContent::CompositionInstance(_) => "Composition Instance",
        NodeContent::PluginOperation(operation) => match operation.category.as_str() {
            TRANSFORM_CATEGORY => "Transform",
            category if path_effect::is_category(category) => "Path Effect",
            "decorator" => "Decorator",
            "effector" => "Effector",
            "style" => "Style",
            "effect" => "Effect",
            _ => "Plug-in",
        },
        NodeContent::Value(value) => value.label(),
        NodeContent::Merge => "Composite",
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "clip inspection requires selection, model, UI, timing, and history context"
)]
fn render_node(
    ui: &mut Ui,
    node: &Node,
    composition_id: Uuid,
    track_id: Option<Uuid>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label("Type:");
        ui.label(node_display_type(node));
    });
    let state = if !node.enabled {
        "Disabled"
    } else if node.bypassed {
        "Bypassed"
    } else {
        "Enabled"
    };
    let state_response = ui.horizontal(|ui| {
        ui.label("State:");
        ui.label(state)
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.node_state:{}", node.id),
        "inspector_node_state",
        state_response.response.rect,
        true,
        Some(serde_json::json!({
            "node_id": node.id,
            "enabled": node.enabled,
            "bypassed": node.bypassed,
            "supports_bypass": node.supports_bypass(),
        })),
    );

    path_effect::render_contract(ui, node);

    render_node_properties(
        ui,
        node,
        composition_id,
        track_id,
        current_time,
        fps,
        resolution,
        project_service,
        history_manager,
        editor_context,
        needs_refresh,
    );

    // A directly selected Node is a focused view; appearance and processing are separate operation Nodes exposed
    // by the owning Clip/Track/Composition facade, never by legacy embedded
    // arrays that would create a second write path.
}

#[allow(
    clippy::too_many_arguments,
    reason = "property rendering requires the authoritative owner, timing, history, and UI context"
)]
fn render_node_properties(
    ui: &mut Ui,
    node: &Node,
    composition_id: Uuid,
    track_id: Option<Uuid>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    let descriptor_definitions = canonical_value_property_definitions(node).or_else(|| {
        plugin_operation_property_definitions(project_service.get_plugin_manager().as_ref(), node)
    });
    let mut definitions = descriptor_definitions.unwrap_or_else(|| {
        project_service.get_property_definitions(
            composition_id,
            track_id.unwrap_or_else(Uuid::nil),
            node.id,
        )
    });
    let known_names: HashSet<String> = definitions
        .iter()
        .map(|definition| definition.name().to_owned())
        .collect();
    definitions.extend(
        inferred_property_definitions(node.properties(), current_time)
            .into_iter()
            .filter(|definition| !known_names.contains(definition.name())),
    );

    if !definitions.is_empty() {
        ui.add_space(5.0);
        ui.heading("Properties");
        render_property_map(
            ui,
            project_service,
            history_manager,
            editor_context,
            PropertyOwner::Node(node.id),
            node.properties(),
            definitions,
            current_time,
            fps,
            resolution,
            needs_refresh,
        );
    }
}

fn canonical_value_property_definitions(node: &Node) -> Option<Vec<PropertyDefinition>> {
    let NodeContent::Value(value) = node.content() else {
        return None;
    };
    Some(value.property_definitions().to_vec())
}

fn plugin_operation_property_definitions(
    plugin_manager: &PluginManager,
    node: &Node,
) -> Option<Vec<PropertyDefinition>> {
    let NodeContent::PluginOperation(operation) = node.content() else {
        return None;
    };
    match plugin_manager.operation_descriptor(
        &operation.category,
        &operation.component_id,
        &operation.operation,
    ) {
        Ok(descriptor) => Some(descriptor.properties().to_vec()),
        Err(error) => {
            // Projects stay loadable without the plugin. In that case the
            // Inspector falls back to value inference, but installed
            // operations always use their authoritative ranges and widgets.
            log::warn!(
                "Cannot resolve Inspector metadata for {}/{}/{}: {error}",
                operation.category,
                operation.component_id,
                operation.operation,
            );
            None
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "node inspection requires selection, model, UI, timing, and history context"
)]
fn render_property_map(
    ui: &mut Ui,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    owner: PropertyOwner,
    properties: &PropertyMap,
    definitions: Vec<PropertyDefinition>,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    needs_refresh: &mut bool,
) {
    struct Chunk {
        in_grid: bool,
        definitions: Vec<PropertyDefinition>,
    }

    let qa_scope = qa_owner_scope(owner);
    let evaluated =
        evaluate_property_map(project_service, properties, current_time, fps, resolution);
    render_evaluation_issues(ui, &qa_scope, evaluated.issues());

    let mut chunks = Vec::new();
    let mut grid_definitions = Vec::new();
    for definition in definitions {
        if matches!(definition.ui_type(), PropertyUiType::MultilineText) {
            if !grid_definitions.is_empty() {
                chunks.push(Chunk {
                    in_grid: true,
                    definitions: std::mem::take(&mut grid_definitions),
                });
            }
            chunks.push(Chunk {
                in_grid: false,
                definitions: vec![definition],
            });
        } else {
            grid_definitions.push(definition);
        }
    }
    if !grid_definitions.is_empty() {
        chunks.push(Chunk {
            in_grid: true,
            definitions: grid_definitions,
        });
    }

    for (chunk_index, chunk) in chunks.iter().enumerate() {
        let context = PropertyRenderContext {
            available_fonts: &editor_context.available_fonts,
            in_grid: chunk.in_grid,
            current_time,
            qa_scope: qa_scope.clone(),
        };
        let actions = if chunk.in_grid {
            let mut actions = Vec::new();
            egui::Grid::new(("inspector_properties", owner, chunk_index))
                .striped(true)
                .show(ui, |ui| {
                    actions = render_property_rows(
                        ui,
                        &chunk.definitions,
                        |name| evaluated.value(name).cloned(),
                        |name| properties.get(name).cloned(),
                        &context,
                    );
                });
            actions
        } else {
            ui.add_space(5.0);
            render_property_rows(
                ui,
                &chunk.definitions,
                |name| evaluated.value(name).cloned(),
                |name| properties.get(name).cloned(),
                &context,
            )
        };

        let mut context = ActionContext::new(project_service, history_manager, owner, current_time);
        if context.handle_actions(actions, |name| properties.get(name).cloned()) {
            *needs_refresh = true;
        }
    }
}

fn qa_owner_scope(owner: PropertyOwner) -> String {
    match owner {
        PropertyOwner::Clip(id) => format!("clip:{id}"),
        PropertyOwner::Node(id) => format!("node:{id}"),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "property sections share owner, model, UI, timing, and history context"
)]
fn render_clip_timing(
    ui: &mut Ui,
    clip: &Clip,
    fps: f64,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
    needs_refresh: &mut bool,
) {
    ui.add_space(5.0);
    ui.heading("Timing");
    ui.separator();

    egui::Grid::new(("clip_timing", clip.id))
        .striped(true)
        .show(ui, |ui| {
            let fps = if fps.is_finite() && fps > 0.0 {
                fps
            } else {
                1.0
            };
            let (
                Some(start_definition),
                Some(duration_definition),
                Some(trim_definition),
                Some(stretch_definition),
            ) = (
                Clip::timing_property_definition(CLIP_START_TIME_PROPERTY),
                Clip::timing_property_definition(CLIP_DURATION_PROPERTY),
                Clip::timing_property_definition(CLIP_TRIM_IN_PROPERTY),
                Clip::timing_property_definition(CLIP_TIME_STRETCH_PROPERTY),
            )
            else {
                log::error!("Clip timing definitions are incomplete");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Clip timing metadata is incomplete.",
                );
                return;
            };
            let start_frame = clip.start_time.into_inner() * fps;
            let duration_frame = clip.duration.into_inner() * fps;
            let trim_in_frame = clip.trim_in.into_inner() * fps;
            let (
                Some(start_config),
                Some(duration_config),
                Some(trim_config),
                Some(stretch_config),
            ) = (
                inspector_timing_drag_config(start_definition, fps, 0.0),
                inspector_timing_drag_config(duration_definition, fps, start_frame),
                inspector_timing_drag_config(trim_definition, fps, 0.0),
                FloatDragValueConfig::from_definition(stretch_definition),
            )
            else {
                log::error!("Clip timing definitions do not use Float UI metadata");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Clip timing controls have invalid metadata.",
                );
                return;
            };

            ui.label(format!("{} Frame", start_definition.label()));
            let mut edited_start = start_frame;
            let response = ui.add(start_config.widget(&mut edited_start));
            register_clip_timing_control(
                clip.id,
                start_definition,
                &response,
                clip.start_time.into_inner(),
                start_frame,
                fps,
                "frame",
            );
            if response.changed() {
                if let Err(error) = project_service.update_clip_timing(
                    clip.id,
                    edited_start / fps,
                    clip.duration.into_inner(),
                    clip.trim_in.into_inner(),
                ) {
                    log::error!("Failed to update Clip start: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label("Out Frame");
            let mut edited_end = start_frame + duration_frame;
            let response = ui.add(duration_config.widget(&mut edited_end));
            register_clip_timing_control(
                clip.id,
                duration_definition,
                &response,
                clip.duration.into_inner(),
                start_frame + duration_frame,
                fps,
                "out_frame",
            );
            if response.changed() {
                let duration = edited_end / fps - clip.start_time.into_inner();
                if let Err(error) = project_service.update_clip_timing(
                    clip.id,
                    clip.start_time.into_inner(),
                    duration,
                    clip.trim_in.into_inner(),
                ) {
                    log::error!("Failed to update Clip duration: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label(format!("{} Frame", trim_definition.label()));
            let mut edited_trim = trim_in_frame;
            let response = ui.add(trim_config.widget(&mut edited_trim));
            register_clip_timing_control(
                clip.id,
                trim_definition,
                &response,
                clip.trim_in.into_inner(),
                trim_in_frame,
                fps,
                "frame",
            );
            if response.changed() {
                if let Err(error) = project_service.update_clip_property(
                    clip.id,
                    trim_definition.name(),
                    PropertyValue::Number(OrderedFloat(edited_trim / fps)),
                ) {
                    log::error!("Failed to update Clip source start: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label(stretch_definition.label());
            let mut edited_stretch = clip.time_stretch.into_inner();
            let response = ui.add(stretch_config.widget(&mut edited_stretch));
            register_clip_timing_control(
                clip.id,
                stretch_definition,
                &response,
                clip.time_stretch.into_inner(),
                edited_stretch,
                fps,
                "ratio",
            );
            if response.changed() {
                if let Err(error) = project_service.update_clip_property(
                    clip.id,
                    stretch_definition.name(),
                    PropertyValue::Number(OrderedFloat(edited_stretch)),
                ) {
                    log::error!("Failed to update Clip time stretch: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label(duration_definition.label());
            ui.label(format!("{duration_frame:.0} fr"));
            ui.end_row();
        });
}

fn register_clip_timing_control(
    clip_id: Uuid,
    definition: &PropertyDefinition,
    response: &egui::Response,
    value: f64,
    display_value: f64,
    fps: f64,
    display_semantics: &str,
) {
    if !crate::qa::is_enabled() {
        return;
    }

    crate::qa::register_component_with_metadata(
        format!("inspector.property.clip:{clip_id}:{}", definition.name()),
        "inspector_property_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "scope": format!("clip:{clip_id}"),
            "property": definition.name(),
            "control_kind": "float_drag",
            "value": value,
            "display_value": display_value,
            "display_semantics": display_semantics,
            "fps": fps,
            "definition": properties::property_definition_metadata(definition),
        })),
    );
}

fn inspector_timing_drag_config(
    definition: &PropertyDefinition,
    fps: f64,
    frame_offset: f64,
) -> Option<FloatDragValueConfig> {
    FloatDragValueConfig::from_definition(definition)
        .map(|config| config.transformed(fps, frame_offset, " fr"))
}

fn commit_timing_edit(
    response: &egui::Response,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    if !(response.drag_stopped() || response.lost_focus()) {
        return;
    }
    match project.read() {
        Ok(project) => history_manager.push_project_state(project.clone()),
        Err(error) => log::error!("Failed to save Clip timing history: {error}"),
    }
}

fn node_display_type(node: &Node) -> String {
    match node.content() {
        NodeContent::Media(_) => "Media".to_string(),
        NodeContent::Generator(generator) => match generator {
            GeneratorContent::Shape => "Shape".to_string(),
            GeneratorContent::Text => "Text".to_string(),
            GeneratorContent::Solid => "Solid".to_string(),
            GeneratorContent::SkSL => "SkSL Shader".to_string(),
        },
        NodeContent::CompositionInstance(_) => "Composition Instance".to_string(),
        NodeContent::PluginOperation(operation)
            if operation.category.as_str() == TRANSFORM_CATEGORY =>
        {
            "Transform".to_string()
        }
        NodeContent::PluginOperation(operation)
            if path_effect::is_category(&operation.category) =>
        {
            "Path Effect · Path geometry only".to_string()
        }
        NodeContent::PluginOperation(operation) => format!(
            "Plugin Operation · {} / {}",
            operation.category, operation.operation
        ),
        NodeContent::Value(value) => value.label().to_string(),
        NodeContent::Merge => "Merge".to_string(),
    }
}

fn is_clip_timing_property(name: &str) -> bool {
    matches!(name, "start_time" | "duration" | "trim_in" | "time_stretch")
}

#[cfg(test)]
mod tests;
