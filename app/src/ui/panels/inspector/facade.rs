use std::collections::HashSet;

use egui::Ui;
use library::model::project::{
    ContainerGraphSemantics, PortOwner, ProjectConnection, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::{GeneratorContent, Node, NodeContent};
use library::plugin::TRANSFORM_CATEGORY;
use library::EditorService;
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::node_inspector::render_node_properties;
use super::path_effect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FacadeOwnerKind {
    Composition,
    Track,
}

pub(super) const OPERATION_CATEGORY_SECTIONS: [(&str, &str, &str); 6] = [
    (TRANSFORM_CATEGORY, "Transform", "Root placement"),
    path_effect::CATEGORY_SECTION,
    ("decorator", "Decorator", "Shape modifier"),
    ("effector", "Effector", "Shape modifier"),
    ("style", "Style", "Appearance"),
    ("effect", "Effect", "Image effect"),
];

impl FacadeOwnerKind {
    pub(super) fn qa_value(self) -> &'static str {
        match self {
            Self::Composition => "composition",
            Self::Track => "track",
        }
    }

    pub(super) fn output_mode(self, output_node_id: Option<Uuid>) -> FacadeOutputMode {
        FacadeOutputMode::TimelineChildren(output_node_id)
    }

    fn derived_children_label(self) -> Option<&'static str> {
        match self {
            Self::Composition => Some("ordered child Tracks"),
            Self::Track => Some("ordered child Clips"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FacadeOutputMode {
    TimelineChildren(Option<Uuid>),
}

impl FacadeOutputMode {
    pub(super) fn qa_value(self) -> &'static str {
        "timeline_children"
    }

    fn explicit_node_id(self) -> Option<Uuid> {
        let Self::TimelineChildren(node_id) = self;
        node_id
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the Timeline facade renders one authoritative graph snapshot with editing, timing, history, and QA context"
)]
pub(super) fn render_semantic_graph_facade(
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

pub(super) fn semantic_visual_sources(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|node| node.content().is_semantic_visual_source())
        .collect()
}

pub(super) fn native_value_nodes(nodes: &[Node]) -> Vec<&Node> {
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

pub(super) fn facade_output_text(
    owner_kind: FacadeOwnerKind,
    output_mode: FacadeOutputMode,
    _nodes: &[Node],
) -> String {
    match output_mode {
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
    }
}

pub(super) fn facade_output_metadata(
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

pub(super) fn is_content_flow_connection(connection: &ProjectConnection) -> bool {
    matches!(
        (connection.from.port.as_str(), connection.to.port.as_str()),
        (IMAGE_OUTPUT_PORT, IMAGE_INPUT_PORT | MERGE_IMAGES_PORT)
            | (SHAPE_OUTPUT_PORT, SHAPE_INPUT_PORT)
    )
}

pub(super) fn content_connection_metadata(connection: &ProjectConnection) -> serde_json::Value {
    serde_json::json!({
        "connection_id": connection.id,
        "from_owner": connection.from.owner,
        "from_port": connection.from.port,
        "to_owner": connection.to.owner,
        "to_port": connection.to.port,
        "order": connection.order,
    })
}

pub(super) fn operation_category(node: &Node) -> Option<&str> {
    let NodeContent::PluginOperation(operation) = node.content() else {
        return None;
    };
    Some(&operation.category)
}

pub(super) fn operation_state_label(
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

pub(super) fn source_kind(node: &Node) -> &'static str {
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
