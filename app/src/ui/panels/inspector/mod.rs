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
mod presentation;
pub mod properties;
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

const OPERATION_CATEGORY_SECTIONS: [(&str, &str, &str); 5] = [
    (TRANSFORM_CATEGORY, "Transform", "Root placement"),
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
        match output_node_id {
            Some(node_id) => FacadeOutputMode::Explicit(node_id),
            None => match self {
                Self::Composition | Self::Track => FacadeOutputMode::DerivedChildren,
                Self::Clip => FacadeOutputMode::NoOutput,
            },
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
    DerivedChildren,
    NoOutput,
}

impl FacadeOutputMode {
    fn qa_value(self) -> &'static str {
        match self {
            Self::Explicit(_) => "explicit",
            Self::DerivedChildren => "derived_children",
            Self::NoOutput => "no_output",
        }
    }

    fn explicit_node_id(self) -> Option<Uuid> {
        match self {
            Self::Explicit(node_id) => Some(node_id),
            Self::DerivedChildren | Self::NoOutput => None,
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
        .filter(|node| {
            matches!(
                node.content(),
                NodeContent::Media(_) | NodeContent::Generator(_) | NodeContent::Reference(_)
            )
        })
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
                "is_result": is_result,
                "structurally_reaches_result": wired_to_result,
                "connection_count": outgoing.len(),
                "connections": connection_metadata,
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
        FacadeOutputMode::DerivedChildren => format!(
            "Derived from {}",
            owner_kind
                .derived_children_label()
                .unwrap_or("ordered child containers")
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
        "explicit": matches!(output_mode, FacadeOutputMode::Explicit(_)),
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
    is_result: bool,
    wired_to_result: bool,
    outgoing: &[&ProjectConnection],
) -> String {
    if !available {
        return "Unavailable".to_string();
    }
    if !enabled {
        return "Disabled".to_string();
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
        NodeContent::Reference(_) => "Reference",
        NodeContent::PluginOperation(operation) => match operation.category.as_str() {
            TRANSFORM_CATEGORY => "Transform",
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

    // A directly selected Node is a focused view of that authoritative Node.
    // Appearance and processing are separate operation Nodes and are exposed
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
        NodeContent::Reference(_) => "Reference".to_string(),
        NodeContent::PluginOperation(operation)
            if operation.category.as_str() == TRANSFORM_CATEGORY =>
        {
            "Transform".to_string()
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
mod tests {
    use super::*;
    use crate::test_support::generator_node;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::project::NodeContainer;
    use library::model::property::Property;
    use library::plugin::{
        EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID,
        TRANSFORM_APPLY_OPERATION,
    };

    #[test]
    fn clip_selection_keeps_every_contained_node_in_order() {
        let mut project = Project::new("inspector");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let first = Node::new_merge("first");
        let second = Node::new_merge("second");
        let mut clip = Clip::new("clip", 2.0, 4.0);
        let clip_id = clip.id;
        let first_id = first.id;
        let second_id = second.id;
        clip.node_ids = vec![second_id, first_id];
        clip.output_node_id = Some(first_id);
        project.add_node(first);
        project.add_node(second);
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let Some(InspectorSelection::Clip { nodes, .. }) = resolve_selection(
            &project,
            Some(SelectionTarget::Clip(clip_id)),
            composition_id,
        ) else {
            panic!("Clip selection should resolve");
        };
        assert_eq!(
            nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![second_id, first_id]
        );
    }

    #[test]
    fn direct_node_selection_stays_node_owned() {
        let mut project = Project::new("inspector");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let node = Node::new_merge("leaf");
        let node_id = node.id;
        let clip = Clip::new("clip", 3.0, 5.0);
        let clip_id = clip.id;
        project.add_node(node);
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();

        let Some(InspectorSelection::Node {
            node,
            containing_clip,
            ..
        }) = resolve_selection(
            &project,
            Some(SelectionTarget::Node(node_id)),
            composition_id,
        )
        else {
            panic!("Node selection should resolve");
        };
        assert_eq!(node.id, node_id);
        assert_eq!(containing_clip.unwrap().id, clip_id);
    }

    #[test]
    fn same_uuid_node_and_clip_resolve_by_explicit_target_kind() {
        let mut project = Project::new("same UUID inspector");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let shared_id = Uuid::new_v4();
        let mut clip = Clip::new("clip with shared UUID", 0.0, 5.0);
        clip.id = shared_id;
        let mut node = Node::new_merge("node with shared UUID");
        node.id = shared_id;

        project.add_track(track);
        project.add_composition(composition);
        project.add_clip(clip);
        project.add_node(node);
        project.attach_clip_to_track(track_id, shared_id).unwrap();
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), shared_id)
            .unwrap();

        let Some(InspectorSelection::Clip { clip, .. }) = resolve_selection(
            &project,
            Some(SelectionTarget::Clip(shared_id)),
            composition_id,
        ) else {
            panic!("typed Clip target should resolve the Clip registry");
        };
        assert_eq!(clip.name, "clip with shared UUID");

        let Some(InspectorSelection::Node { node, track_id, .. }) = resolve_selection(
            &project,
            Some(SelectionTarget::Node(shared_id)),
            composition_id,
        ) else {
            panic!("typed Node target should resolve the Node registry");
        };
        assert_eq!(node.name, "node with shared UUID");
        assert_eq!(track_id, None);
    }

    #[test]
    fn timeline_track_and_composition_resolve_without_a_leaf_selection() {
        let mut project = Project::new("timeline owner inspector");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 5.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);

        let Some(InspectorSelection::Track { track, .. }) = resolve_selection(
            &project,
            Some(SelectionTarget::Track(track_id)),
            composition_id,
        ) else {
            panic!("Track selection should resolve");
        };
        assert_eq!(track.id, track_id);

        let Some(InspectorSelection::Composition { composition, .. }) =
            resolve_selection(&project, None, composition_id)
        else {
            panic!("Composition selection should resolve");
        };
        assert_eq!(composition.id, composition_id);
    }

    #[test]
    fn explicit_selection_from_another_composition_does_not_fall_back() {
        let mut project = Project::new("composition scoped inspector");
        let (active, active_track) = Composition::new("active", 1920, 1080, 30.0, 5.0);
        let active_id = active.id;
        project.add_track(active_track);
        project.add_composition(active);

        let (other, other_track) = Composition::new("other", 1920, 1080, 30.0, 5.0);
        let other_track_id = other_track.id;
        let mut other_clip = Clip::new("other clip", 0.0, 1.0);
        let other_clip_id = other_clip.id;
        let other_node = Node::new_merge("other node");
        let other_node_id = other_node.id;
        other_clip.node_ids.push(other_node_id);
        project.add_track(other_track);
        project.add_composition(other);
        project.add_node(other_node);
        project.add_clip(other_clip);
        project
            .attach_clip_to_track(other_track_id, other_clip_id)
            .unwrap();

        assert!(resolve_selection(
            &project,
            Some(SelectionTarget::Node(other_node_id)),
            active_id,
        )
        .is_none());
    }

    #[test]
    fn structural_status_reuses_the_authoritative_clip_semantics(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::new("inspector graph semantics");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let clip = Clip::new("clip", 0.0, 10.0);
        let clip_id = clip.id;
        let source = generator_node(
            "Title",
            GeneratorNodeRequest::Text {
                text: "Title".to_string(),
                font: "Arial".to_string(),
            },
        );
        let plugin_manager = PluginManager::default();
        let applied = plugin_manager.create_style_operation_node("fill")?;
        let disconnected = plugin_manager.create_effect_operation_node("blur")?;
        let result = Node::new_merge("Composite");
        let source_id = source.id;
        let applied_id = applied.id;
        let disconnected_id = disconnected.id;
        let result_id = result.id;

        project.add_track(track);
        project.add_composition(composition);
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id)?;
        for node in [source, applied, disconnected, result] {
            let node_id = node.id;
            project.add_node(node);
            project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id)?;
        }
        project.connect_ports(
            library::model::project::PortAddress::new(
                PortOwner::Node(source_id),
                SHAPE_OUTPUT_PORT,
            ),
            library::model::project::PortAddress::new(
                PortOwner::Node(applied_id),
                SHAPE_INPUT_PORT,
            ),
        )?;
        let result_connection_id = project.connect_ports(
            library::model::project::PortAddress::new(
                PortOwner::Node(applied_id),
                IMAGE_OUTPUT_PORT,
            ),
            library::model::project::PortAddress::new(
                PortOwner::Node(result_id),
                MERGE_IMAGES_PORT,
            ),
        )?;
        project.set_output_node(NodeContainer::Clip(clip_id), Some(result_id))?;
        let Some(result_connection) = project
            .connections
            .iter_mut()
            .find(|connection| connection.id == result_connection_id)
        else {
            return Err(std::io::Error::other("result connection was not retained").into());
        };
        result_connection.order = 3;
        project.connections.push(ProjectConnection::new(
            library::model::project::PortAddress::new(
                PortOwner::Node(applied_id),
                "property.opacity",
            ),
            library::model::project::PortAddress::new(
                PortOwner::Node(disconnected_id),
                "property.sigma_x",
            ),
            99,
        ));

        let Some(InspectorSelection::Clip {
            semantics,
            connections,
            ..
        }) = resolve_selection(
            &project,
            Some(SelectionTarget::Clip(clip_id)),
            composition_id,
        )
        else {
            return Err(std::io::Error::other("Clip selection should resolve").into());
        };
        assert_eq!(
            semantics,
            project.container_graph_semantics(PortOwner::Clip(clip_id))
        );
        assert!(semantics.structurally_reaches_output(PortOwner::Node(source_id)));
        assert!(semantics.structurally_reaches_output(PortOwner::Node(applied_id)));
        assert!(semantics.structurally_reaches_output(PortOwner::Node(result_id)));
        assert!(!semantics.structurally_reaches_output(PortOwner::Node(disconnected_id)));
        let outgoing = connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Node(applied_id)
                    && is_content_flow_connection(connection)
            })
            .collect::<Vec<_>>();
        assert_eq!(outgoing.len(), 1, "scalar wires are not semantic branches");
        let metadata = content_connection_metadata(outgoing[0]);
        assert_eq!(metadata["connection_id"], serde_json::json!(outgoing[0].id));
        assert_eq!(
            metadata["from_owner"],
            serde_json::json!(PortOwner::Node(applied_id))
        );
        assert_eq!(metadata["from_port"], IMAGE_OUTPUT_PORT);
        assert_eq!(
            metadata["to_owner"],
            serde_json::json!(PortOwner::Node(result_id))
        );
        assert_eq!(metadata["to_port"], MERGE_IMAGES_PORT);
        assert_eq!(metadata["order"], 3);
        assert_eq!(
            operation_state_label(true, true, false, true, &outgoing),
            "Wired to result · order 3"
        );
        assert_eq!(
            operation_state_label(true, true, false, false, &[]),
            "Not wired to result"
        );
        assert_eq!(
            operation_state_label(false, true, false, true, &outgoing),
            "Unavailable"
        );
        assert_eq!(
            operation_state_label(true, false, false, true, &outgoing),
            "Disabled"
        );
        Ok(())
    }

    #[test]
    fn facade_output_mode_distinguishes_explicit_children_and_no_output() {
        let result = Node::new_merge("Composite");
        let nodes = [result.clone()];

        for owner_kind in [
            FacadeOwnerKind::Composition,
            FacadeOwnerKind::Track,
            FacadeOwnerKind::Clip,
        ] {
            let output_mode = owner_kind.output_mode(Some(result.id));
            assert_eq!(output_mode, FacadeOutputMode::Explicit(result.id));
            assert_eq!(output_mode.qa_value(), "explicit");
            assert_eq!(
                facade_output_text(owner_kind, output_mode, &nodes),
                "Result: Composite"
            );
            let metadata = facade_output_metadata(owner_kind, output_mode, true);
            assert_eq!(metadata["owner_kind"], owner_kind.qa_value());
            assert_eq!(metadata["output_mode"], "explicit");
            assert_eq!(metadata["output_node_id"], serde_json::json!(result.id));
            assert_eq!(metadata["explicit"], true);
            assert_eq!(metadata["explicit_output_is_directly_contained"], true);
        }

        let composition_mode = FacadeOwnerKind::Composition.output_mode(None);
        assert_eq!(composition_mode, FacadeOutputMode::DerivedChildren);
        assert_eq!(composition_mode.qa_value(), "derived_children");
        assert_eq!(
            facade_output_text(FacadeOwnerKind::Composition, composition_mode, &nodes,),
            "Derived from ordered child Tracks"
        );
        let composition_metadata =
            facade_output_metadata(FacadeOwnerKind::Composition, composition_mode, false);
        assert_eq!(composition_metadata["output_mode"], "derived_children");
        assert_eq!(
            composition_metadata["output_node_id"],
            serde_json::Value::Null
        );
        assert_eq!(composition_metadata["explicit"], false);

        let track_mode = FacadeOwnerKind::Track.output_mode(None);
        assert_eq!(track_mode, FacadeOutputMode::DerivedChildren);
        assert_eq!(track_mode.qa_value(), "derived_children");
        assert_eq!(
            facade_output_text(FacadeOwnerKind::Track, track_mode, &nodes),
            "Derived from ordered child Clips"
        );
        let track_metadata = facade_output_metadata(FacadeOwnerKind::Track, track_mode, false);
        assert_eq!(track_metadata["owner_kind"], "track");
        assert_eq!(track_metadata["output_mode"], "derived_children");

        let clip_mode = FacadeOwnerKind::Clip.output_mode(None);
        assert_eq!(clip_mode, FacadeOutputMode::NoOutput);
        assert_eq!(clip_mode.qa_value(), "no_output");
        assert_eq!(
            facade_output_text(FacadeOwnerKind::Clip, clip_mode, &nodes),
            "No output selected (NoOutput)"
        );
        let clip_metadata = facade_output_metadata(FacadeOwnerKind::Clip, clip_mode, false);
        assert_eq!(clip_metadata["owner_kind"], "clip");
        assert_eq!(clip_metadata["output_mode"], "no_output");
        assert_eq!(clip_metadata["output_node_id"], serde_json::Value::Null);
        assert_eq!(clip_metadata["explicit"], false);
    }

    #[test]
    fn inferred_definitions_cover_editable_values_and_skip_structures() {
        let mut properties = PropertyMap::new();
        properties.set(
            "gain".into(),
            Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
        );
        properties.set(
            "display_name".into(),
            Property::constant(PropertyValue::String("Title".into())),
        );
        properties.set(
            "metadata".into(),
            Property::constant(PropertyValue::Map(Default::default())),
        );

        let definitions = inferred_property_definitions(&properties, 0.0);
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.name())
                .collect::<Vec<_>>(),
            vec!["display_name", "gain"]
        );
        assert_eq!(property_label("display_name"), "Display Name");
    }

    #[test]
    fn installed_plugin_operation_uses_authoritative_inspector_ranges() {
        let plugins = PluginManager::default();
        let node = plugins.create_style_operation_node("stroke").unwrap();
        let definitions = plugin_operation_property_definitions(&plugins, &node)
            .expect("installed operation descriptor");
        let width = definitions
            .iter()
            .find(|definition| definition.name() == "width")
            .expect("Stroke width definition");
        assert!(matches!(
            width.ui_type(),
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix,
                min_hard_limit: false,
                max_hard_limit: false,
            } if suffix == "px"
        ));
        assert_eq!(width.default_value(), &PropertyValue::from(1.0));
        let join = definitions
            .iter()
            .find(|definition| definition.name() == "join")
            .expect("Stroke join definition");
        assert!(matches!(
            join.ui_type(),
            PropertyUiType::Dropdown { options }
                if options == &["Miter".to_string(), "Round".to_string(), "Bevel".to_string()]
        ));
        assert_eq!(
            join.default_value(),
            &PropertyValue::String("Round".to_string())
        );

        let inferred = inferred_property_definitions(node.properties(), 0.0);
        let inferred_width = inferred
            .iter()
            .find(|definition| definition.name() == "width")
            .unwrap();
        assert!(matches!(
            inferred_width.ui_type(),
            PropertyUiType::Float {
                min: -1_000_000.0,
                max: 1_000_000.0,
                ..
            }
        ));
        assert_ne!(width.ui_type(), inferred_width.ui_type());
    }

    #[test]
    fn fmod_uses_canonical_divisor_metadata_instead_of_inferred_ranges() {
        let node = Node::new_fmod("Fmod");
        let definitions = canonical_value_property_definitions(&node).unwrap();
        let divisor = definitions
            .iter()
            .find(|definition| definition.name() == "divisor")
            .unwrap();
        assert_eq!(divisor.label(), "Divisor");
        assert_eq!(divisor.default_value(), &PropertyValue::from(1.0));
        assert!(matches!(
            divisor.ui_type(),
            PropertyUiType::Float {
                min: -1_000_000.0,
                max: 1_000_000.0,
                step: 0.01,
                suffix,
                min_hard_limit: false,
                max_hard_limit: false,
            } if suffix.is_empty()
        ));

        let inferred = inferred_property_definitions(node.properties(), 0.0);
        assert_ne!(inferred[0].ui_type(), divisor.ui_type());
    }

    #[test]
    fn root_transform_has_transform_semantics_and_descriptor_property_controls() {
        let plugins = PluginManager::default();
        let node = plugins.create_shape_transform_operation_node().unwrap();
        let NodeContent::PluginOperation(operation) = node.content() else {
            panic!("Transform factory returned a PluginOperation")
        };
        assert_eq!(operation.category, TRANSFORM_CATEGORY);
        assert_eq!(operation.component_id, SHAPE_TRANSFORM_COMPONENT_ID);
        assert_eq!(operation.operation, TRANSFORM_APPLY_OPERATION);
        assert_eq!(operation_category(&node), Some(TRANSFORM_CATEGORY));
        assert_eq!(source_kind(&node), "Transform");
        assert_eq!(node_display_type(&node), "Transform");
        assert!(OPERATION_CATEGORY_SECTIONS.contains(&(
            TRANSFORM_CATEGORY,
            "Transform",
            "Root placement"
        )));

        let definitions = plugin_operation_property_definitions(&plugins, &node)
            .expect("installed Transform descriptor drives generic Inspector controls");
        assert_eq!(
            definitions
                .iter()
                .map(PropertyDefinition::name)
                .collect::<Vec<_>>(),
            vec!["position", "rotation", "scale", "anchor"]
        );
        assert_eq!(definitions.len(), node.properties().iter().count());
        for definition in &definitions {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(|property| property.evaluate_at(0.0).ok()),
                Some(definition.default_value().clone())
            );
        }
        let position = definitions
            .iter()
            .find(|definition| definition.name() == "position")
            .unwrap();
        assert!(matches!(
            position.ui_type(),
            PropertyUiType::Vec2 { suffix, .. } if suffix == "px"
        ));
        let rotation = definitions
            .iter()
            .find(|definition| definition.name() == "rotation")
            .unwrap();
        assert!(matches!(
            rotation.ui_type(),
            PropertyUiType::Float {
                min: -360.0,
                max: 360.0,
                step: 1.0,
                suffix,
                min_hard_limit: false,
                max_hard_limit: false,
            } if suffix == "deg"
        ));
    }

    #[test]
    fn value_nodes_are_numeric_operations_and_never_visual_sources() {
        let source = generator_node(
            "Solid",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        let source_id = source.id;
        let value = Node::new_fmod("Fmod");
        let value_id = value.id;
        let merge = Node::new_merge("Merge");
        let nodes = vec![source, value, merge];

        assert_eq!(
            semantic_visual_sources(&nodes)
                .into_iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![source_id]
        );
        assert_eq!(
            native_value_nodes(&nodes)
                .into_iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![value_id]
        );
        assert_eq!(source_kind(&nodes[1]), "Fmod");
    }

    #[test]
    fn effect_operation_descriptor_drives_inspector_and_qa_metadata() {
        let plugins = PluginManager::default();
        let node = plugins.create_effect_operation_node("blur").unwrap();
        let definitions = plugin_operation_property_definitions(&plugins, &node)
            .expect("installed Effect descriptor");
        let sigma_x = definitions
            .iter()
            .find(|definition| definition.name() == "sigma_x")
            .expect("Blur sigma_x definition");
        assert_eq!(
            properties::property_definition_metadata(sigma_x),
            serde_json::json!({
                "name": "sigma_x",
                "label": "Sigma X",
                "default": 0.0,
                "ui": {
                    "kind": "float",
                    "min": 0.0,
                    "max": 100.0,
                    "step": 0.1,
                    "suffix": "px",
                    "min_hard_limit": true,
                    "max_hard_limit": false,
                },
            })
        );
        let tile_mode = definitions
            .iter()
            .find(|definition| definition.name() == "tile_mode")
            .expect("Blur tile_mode definition");
        assert_eq!(
            properties::property_definition_metadata(tile_mode),
            serde_json::json!({
                "name": "tile_mode",
                "label": "Tile Mode",
                "default": "clamp",
                "ui": {
                    "kind": "dropdown",
                    "options": ["clamp", "repeat", "mirror", "decal"],
                },
            })
        );
    }

    #[test]
    fn effector_descriptor_initializes_and_describes_transform_and_opacity_controls() {
        let plugins = PluginManager::default();
        for component_id in ["transform", "opacity"] {
            let node = plugins
                .create_effector_operation_node(component_id)
                .unwrap();
            let definitions = plugin_operation_property_definitions(&plugins, &node)
                .expect("installed Effector descriptor");
            assert_eq!(definitions.len(), node.properties().iter().count());
            for definition in &definitions {
                assert_eq!(
                    node.properties()
                        .get(definition.name())
                        .and_then(|property| property.evaluate_at(0.0).ok()),
                    Some(definition.default_value().clone()),
                    "{component_id}.{} must be initialized by its descriptor factory",
                    definition.name(),
                );
            }
            let target = definitions
                .iter()
                .find(|definition| definition.name() == "target")
                .expect("Effector target definition");
            assert_eq!(
                properties::property_definition_metadata(target),
                serde_json::json!({
                    "name": "target",
                    "label": "Target",
                    "default": "Block",
                    "ui": {
                        "kind": "dropdown",
                        "options": ["Block", "Line", "Char"],
                    },
                })
            );
        }

        let opacity = plugins.create_effector_operation_node("opacity").unwrap();
        let definitions = plugin_operation_property_definitions(&plugins, &opacity).unwrap();
        let mode = definitions
            .iter()
            .find(|definition| definition.name() == "mode")
            .expect("Opacity mode definition");
        assert_eq!(
            properties::property_definition_metadata(mode),
            serde_json::json!({
                "name": "mode",
                "label": "Mode",
                "default": "Set",
                "ui": {
                    "kind": "dropdown",
                    "options": ["Set", "Add", "Multiply"],
                },
            })
        );
    }

    #[test]
    fn unknown_plugin_operation_roundtrips_and_falls_back_to_lossless_generic_controls() {
        let plugins = PluginManager::default();
        let node = plugins.create_effector_operation_node("opacity").unwrap();
        let node_id = node.id;
        let NodeContent::PluginOperation(operation) = node.content() else {
            panic!("factory returned a PluginOperation")
        };
        let expected_ports = operation.declared_ports.clone();
        let mut encoded_node = serde_json::to_value(node).unwrap();
        let operation = encoded_node["content"]["data"]
            .as_object_mut()
            .expect("serialized PluginOperation content");
        operation.insert(
            "category".to_string(),
            serde_json::Value::String(EFFECTOR_CATEGORY.to_string()),
        );
        operation.insert(
            "component_id".to_string(),
            serde_json::Value::String("third.party.unavailable-opacity".to_string()),
        );
        operation.insert(
            "operation".to_string(),
            serde_json::Value::String(EFFECTOR_APPLY_OPERATION.to_string()),
        );
        let node: Node = serde_json::from_value(encoded_node).unwrap();
        let expected_node = node.clone();

        let mut project = Project::new("foreign plugin roundtrip");
        project.add_node(node);
        let encoded = serde_json::to_value(&project).unwrap();
        let decoded: Project = serde_json::from_value(encoded).unwrap();
        let restored = decoded
            .get_node(node_id)
            .expect("roundtripped operation Node");
        assert_eq!(restored, &expected_node);
        let NodeContent::PluginOperation(restored_operation) = restored.content() else {
            panic!("roundtripped PluginOperation identity")
        };
        assert_eq!(restored_operation.declared_ports, expected_ports);
        assert!(plugin_operation_property_definitions(&plugins, restored).is_none());
        assert_eq!(
            node_display_type(restored),
            format!(
                "Plugin Operation · {} / {}",
                EFFECTOR_CATEGORY, EFFECTOR_APPLY_OPERATION
            )
        );
        let fallback = inferred_property_definitions(restored.properties(), 0.0);
        assert_eq!(fallback.len(), restored.properties().iter().count());
        for property_name in ["opacity", "mode", "target"] {
            assert!(
                fallback
                    .iter()
                    .any(|definition| definition.name() == property_name),
                "unknown plugin value {property_name} must remain generically inspectable"
            );
        }
    }

    #[test]
    fn node_and_inspector_timing_adapters_derive_from_the_same_clip_metadata() {
        let duration = Clip::timing_property_definition("duration").unwrap();
        let node = crate::ui::panels::node_editor::node_timing_drag_config(duration).unwrap();
        let inspector = inspector_timing_drag_config(duration, 30.0, 120.0).unwrap();

        assert_eq!(inspector.speed, node.speed * 30.0);
        assert_eq!(
            inspector.hard_min,
            node.hard_min.map(|min| min * 30.0 + 120.0)
        );
        assert_eq!(
            inspector.hard_max,
            node.hard_max.map(|max| max * 30.0 + 120.0)
        );

        let stretch = Clip::timing_property_definition("time_stretch").unwrap();
        let node_stretch =
            crate::ui::panels::node_editor::node_timing_drag_config(stretch).unwrap();
        assert_eq!(node_stretch.hard_min, Some(0.0));
        assert!(stretch
            .validate_value(&PropertyValue::Number(OrderedFloat(0.0)))
            .is_ok());
    }
}
