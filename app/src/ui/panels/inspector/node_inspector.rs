use std::collections::HashSet;

use egui::Ui;
use library::model::property::{
    ColorSpaceRef, ColorValue, PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue,
};
use library::model::{ColorContent, GeneratorContent, Node, NodeContent};
use library::plugin::{PluginManager, TRANSFORM_CATEGORY};
use library::{EditorService, PropertyOwner};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::action_handler::ActionContext;
use super::evaluation::{evaluate_property_map, render_evaluation_issues};
use super::path_effect;
use super::properties::{PropertyRenderContext, render_property_rows};
use super::property_authoring::PropertyAction;
use super::property_inference::inferred_property_definitions;
use crate::ui::widgets::color_value_picker::color_value_picker;

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
    global_time: f64,
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

    render_legacy_media_color_repair(ui, node, project_service, history_manager, needs_refresh);

    render_node_properties(
        ui,
        node,
        composition_id,
        track_id,
        current_time,
        global_time,
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
    global_time: f64,
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
            .filter(|definition| exact_node_property_is_visible(definition.name(), &known_names)),
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
            global_time,
            fps,
            resolution,
            matches!(node.content(), NodeContent::Color(ColorContent::Compose)),
            needs_refresh,
        );
    }
}

fn exact_node_property_is_visible(name: &str, known_names: &HashSet<String>) -> bool {
    !known_names.contains(name) && !library::model::is_legacy_media_color_property(name)
}

fn render_legacy_media_color_repair(
    ui: &mut Ui,
    node: &Node,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    let active = library::model::active_legacy_media_color_properties(node);
    if active.is_empty() {
        return;
    }
    let details = active
        .iter()
        .map(|property| format!("{}: {}", property.key(), property.authored_state()))
        .collect::<Vec<_>>()
        .join(", ");
    let message = format!(
        "This Media Node is fail-closed because deprecated config-less color fields remain ({details}). Source interpretation now belongs to the Asset. Assign it from the Clip Inspector, then clear these retired fields explicitly."
    );
    let response = ui.colored_label(ui.visuals().error_fg_color, &message);
    crate::qa::register_component_with_metadata(
        format!("inspector.node_legacy_source_color:{}.diagnostic", node.id),
        "node_legacy_source_color_diagnostic",
        response.rect,
        true,
        Some(serde_json::json!({
            "node_id": node.id,
            "properties": active.iter().map(|property| property.key()).collect::<Vec<_>>(),
            "message": message,
            "rendering": "fail_closed",
        })),
    );
    let clear = ui.button("Clear retired Node color fields");
    crate::qa::register_component_with_metadata(
        format!("inspector.node_legacy_source_color:{}.clear", node.id),
        "node_legacy_source_color_clear",
        clear.rect,
        true,
        Some(serde_json::json!({
            "node_id": node.id,
            "explicit_repair": true,
            "asset_assignment_unchanged": true,
        })),
    );
    if clear.clicked() {
        match project_service.clear_legacy_media_node_color_properties(node.id) {
            Ok(()) => {
                match project_service.get_project().read() {
                    Ok(project) => history_manager.push_project_state(project.clone()),
                    Err(error) => {
                        log::error!("Failed to capture legacy color repair history: {error}")
                    }
                }
                *needs_refresh = true;
            }
            Err(error) => log::error!("Failed to clear legacy Media color fields: {error}"),
        }
    }
}

pub(super) fn canonical_native_property_definitions(
    node: &Node,
) -> Option<Vec<PropertyDefinition>> {
    match node.content() {
        NodeContent::Value(value) => Some(value.property_definitions().to_vec()),
        NodeContent::Data(data) => Some(data.property_definitions().to_vec()),
        NodeContent::Color(operation) => Some(operation.property_definitions().to_vec()),
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
    global_time: f64,
    fps: f64,
    resolution: (u64, u64),
    show_compose_picker: bool,
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

    if show_compose_picker {
        let linked_inputs = match owner {
            PropertyOwner::Node(node_id) => project_service
                .get_project()
                .read()
                .map(|project| {
                    crate::utils::property::linked_node_inputs(
                        &project,
                        node_id,
                        &[
                            library::model::COLOR_SPACE_PORT,
                            library::model::COLOR_RED_PORT,
                            library::model::COLOR_GREEN_PORT,
                            library::model::COLOR_BLUE_PORT,
                            library::model::COLOR_ALPHA_PORT,
                        ],
                    )
                })
                .unwrap_or_default(),
            PropertyOwner::Clip(_) => Vec::new(),
        };
        if let Some(actions) = render_compose_picker(
            ui,
            owner,
            &qa_scope,
            &evaluated,
            &linked_inputs,
            project_service,
            global_time,
        ) {
            let mut context =
                ActionContext::new(project_service, history_manager, owner, current_time);
            if context.handle_actions(actions, |name| properties.get(name).cloned()) {
                *needs_refresh = true;
            }
        }
    }

    let mut chunks = Vec::new();
    let mut grid_definitions = Vec::new();
    for definition in definitions {
        if matches!(
            definition.ui_type(),
            PropertyUiType::MultilineText | PropertyUiType::Path
        ) {
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

fn render_compose_picker(
    ui: &mut Ui,
    owner: PropertyOwner,
    qa_scope: &str,
    evaluated: &super::evaluation::EvaluatedPropertyMap,
    linked_inputs: &[(String, library::model::project::PortAddress)],
    project_service: &EditorService,
    global_time: f64,
) -> Option<Vec<PropertyAction>> {
    let value = |key: &str| evaluated.value(key);
    let space = value(library::model::COLOR_SPACE_PORT)?.get_as::<String>()?;
    let component = |key: &str| value(key)?.get_as::<f64>();
    let authored_color = ColorValue::new(
        ColorSpaceRef::new(space).ok()?,
        [
            component(library::model::COLOR_RED_PORT)?,
            component(library::model::COLOR_GREEN_PORT)?,
            component(library::model::COLOR_BLUE_PORT)?,
            component(library::model::COLOR_ALPHA_PORT)?,
        ],
    )
    .ok()?;
    let read_only = !linked_inputs.is_empty();
    let resolved_color = match owner {
        PropertyOwner::Node(node_id) if read_only => project_service
            .get_project()
            .read()
            .ok()
            .and_then(|project| {
                let plugin_manager = project_service.get_plugin_manager();
                match crate::utils::property::evaluate_node_metadata_output(
                    &project,
                    plugin_manager.as_ref(),
                    node_id,
                    library::model::COLOR_VALUE_PORT,
                    global_time,
                ) {
                    Ok(library::model::project::EvalOutput::Produced(
                        PropertyValue::ColorValue(color),
                    )) => Some(color),
                    Ok(_) => None,
                    Err(error) => {
                        log::warn!("Cannot resolve linked Compose color {node_id}: {error}");
                        None
                    }
                }
            }),
        PropertyOwner::Node(_) | PropertyOwner::Clip(_) => None,
    };
    let color = resolved_color.as_ref().unwrap_or(&authored_color);
    ui.label(
        egui::RichText::new(if read_only {
            if resolved_color.is_some() {
                "Linked result color"
            } else {
                "Linked result unavailable · authored fallback"
            }
        } else {
            "Color"
        })
        .strong(),
    );
    let prefix = format!("inspector.aggregate.{qa_scope}:compose_color");
    let mut picker = ui
        .add_enabled_ui(!read_only, |ui| {
            color_value_picker(
                ui,
                egui::Id::new(("inspector_compose_color_picker", owner)),
                color,
            )
        })
        .inner;
    super::properties::structured::register_color_picker(&prefix, color, &picker);
    if read_only {
        let linked = linked_inputs
            .iter()
            .map(|(port, _)| port.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let response = ui
            .label(
                egui::RichText::new(format!("Linked inputs ({linked}) · edit their source Nodes"))
                    .small()
                    .weak(),
            )
            .on_hover_text(
                if resolved_color.is_some() {
                    "The aggregate picker shows the read-only effective runtime Color from the connected input wires."
                } else {
                    "The runtime result is unavailable at this time, so the disabled swatch is explicitly only the authored fallback."
                },
            );
        crate::qa::register_component_with_metadata(
            format!("{prefix}:linked_state"),
            "compose_color_linked_read_only",
            response.rect,
            false,
            Some(serde_json::json!({
                "owner": qa_scope,
                "linked_inputs": linked_inputs.iter().map(|(port, source)| serde_json::json!({
                    "port": port,
                    "source": source,
                })).collect::<Vec<_>>(),
                "editable": false,
                "displayed_value": if resolved_color.is_some() { "resolved_runtime_output" } else { "authored_fallback_unavailable_runtime" },
                "resolved_value": resolved_color.as_ref().map(|color| serde_json::Value::from(&PropertyValue::ColorValue(color.clone()))),
                "runtime_value": "linked_inputs",
            })),
        );
    }

    Some(compose_picker_actions(
        picker.value.take(),
        picker.finished,
        read_only,
    ))
}

fn compose_picker_actions(
    color: Option<ColorValue>,
    finished: bool,
    read_only: bool,
) -> Vec<PropertyAction> {
    if read_only {
        return Vec::new();
    }
    let mut actions = Vec::new();
    if let Some(color) = color {
        let mut values = vec![(
            library::model::COLOR_SPACE_PORT.to_string(),
            PropertyValue::String(color.color_space().to_string()),
        )];
        values.extend(
            [
                library::model::COLOR_RED_PORT,
                library::model::COLOR_GREEN_PORT,
                library::model::COLOR_BLUE_PORT,
                library::model::COLOR_ALPHA_PORT,
            ]
            .into_iter()
            .zip(color.rgba())
            .map(|(key, value)| (key.to_string(), PropertyValue::Number(OrderedFloat(value)))),
        );
        actions.push(PropertyAction::UpdateGroup(values));
    }
    if finished {
        actions.push(PropertyAction::Commit);
    }
    actions
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
        NodeContent::Color(operation) => operation.label().to_string(),
        NodeContent::Data(data) => data.label().to_string(),
        NodeContent::List(operation) => operation.label().to_string(),
        NodeContent::Path(operation) => operation.label().to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_compose_picker_never_authors_fallback_channels() {
        let color = ColorValue::new(ColorSpaceRef::srgb(), [0.2, 0.3, 0.4, 0.5]).unwrap();
        assert!(compose_picker_actions(Some(color), true, true).is_empty());
    }

    #[test]
    fn exact_node_inspector_never_exposes_retired_color_controls() {
        let known = HashSet::new();
        assert!(!exact_node_property_is_visible("input_color_space", &known));
        assert!(!exact_node_property_is_visible(
            "output_color_space",
            &known
        ));
        assert!(exact_node_property_is_visible("file_path", &known));
    }
}
