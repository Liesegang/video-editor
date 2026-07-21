use std::collections::HashSet;

use egui::Ui;
use library::model::property::{PropertyDefinition, PropertyMap, PropertyUiType};
use library::model::{GeneratorContent, Node, NodeContent};
use library::plugin::{PluginManager, TRANSFORM_CATEGORY};
use library::{EditorService, PropertyOwner};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::action_handler::ActionContext;
use super::evaluation::{evaluate_property_map, render_evaluation_issues};
use super::path_effect;
use super::properties::{render_property_rows, PropertyRenderContext};
use super::property_inference::inferred_property_definitions;

#[allow(
    clippy::too_many_arguments,
    reason = "clip inspection requires selection, model, UI, timing, and history context"
)]
pub(super) fn render_node(
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
pub(super) fn render_node_properties(
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
    let descriptor_definitions = canonical_native_property_definitions(node).or_else(|| {
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

pub(super) fn canonical_native_property_definitions(
    node: &Node,
) -> Option<Vec<PropertyDefinition>> {
    match node.content() {
        NodeContent::Value(value) => Some(value.property_definitions().to_vec()),
        NodeContent::List(operation) => Some(operation.property_definitions().to_vec()),
        NodeContent::SoundAnalysis(analysis) => Some(analysis.property_definitions().to_vec()),
        _ => None,
    }
}

pub(super) fn plugin_operation_property_definitions(
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
            show_authoring: true,
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

pub(super) fn node_display_type(node: &Node) -> String {
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
        NodeContent::List(operation) => operation.label().to_string(),
        NodeContent::NativeOperation(operation) => {
            library::model::native_node_descriptor(&operation.catalog_id).map_or_else(
                || format!("Native Operation · unknown ({})", operation.catalog_id),
                |descriptor| {
                    format!(
                        "Native Operation · {} · {}",
                        descriptor.category(),
                        descriptor.runtime_status().key()
                    )
                },
            )
        }
        NodeContent::Merge => "Merge".to_string(),
        NodeContent::SoundMerge => "Sound Merge".to_string(),
        NodeContent::SoundAnalysis(analysis) => analysis.label().to_string(),
    }
}
