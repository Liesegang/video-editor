use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use egui::Ui;
use library::model::node::{
    CLIP_DURATION_PROPERTY, CLIP_START_TIME_PROPERTY, CLIP_TIME_STRETCH_PROPERTY,
    CLIP_TRIM_IN_PROPERTY,
};
use library::model::project::Project;
use library::model::property::{
    PropertyDefinition, PropertyMap, PropertyTarget, PropertyUiType, PropertyValue,
};
use library::model::{Clip, GeneratorContent, Node, NodeContent};
use library::plugin::PluginManager;
use library::{EditorService, PropertyOwner};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::ui::widgets::property_drag_value::FloatDragValueConfig;
use crate::{action::HistoryManager, state::context::EditorContext};

pub mod action_handler;
pub mod effects;
pub mod ensemble;
pub mod properties;
pub mod styles;

use action_handler::ActionContext;
use effects::render_effects_section;
use ensemble::render_ensemble_section;
use properties::{render_property_rows, PropertyRenderContext};
use styles::render_styles_section;

#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "the Inspector takes one short-lived authoritative selection snapshot per frame"
)]
enum InspectorSelection {
    Clip {
        clip: Clip,
        nodes: Vec<Node>,
        track_id: Option<Uuid>,
    },
    Node {
        node: Node,
        track_id: Option<Uuid>,
        containing_clip: Option<Clip>,
    },
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
    let Some(composition_id) = editor_context.selection.composition_id else {
        ui.label("No composition selected.");
        return;
    };
    let Some(selected_entity_id) = editor_context.selection.last_selected_entity_id else {
        ui.label("Select a Clip or Node to edit.");
        return;
    };

    let selection = match project.read() {
        Ok(project) => resolve_selection(&project, selected_entity_id),
        Err(error) => {
            log::error!("Failed to read Project for Inspector: {error}");
            ui.label("Project is temporarily unavailable.");
            return;
        }
    };

    let Some(selection) = selection else {
        ui.label("Selected Clip or Node was not found (it may have been deleted).");
        editor_context.selection.last_selected_entity_id = None;
        editor_context
            .selection
            .selected_entities
            .remove(&selected_entity_id);
        return;
    };

    let fps = project_service
        .get_composition(composition_id)
        .map(|composition| composition.fps)
        .unwrap_or(60.0);
    let global_time = editor_context.timeline.current_time as f64;
    let mut needs_refresh = false;

    render_multi_selection_notice(ui, editor_context);

    match selection {
        InspectorSelection::Clip {
            clip,
            nodes,
            track_id,
        } => {
            let heading = ui.heading(format!("Clip: {}", clip.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.clip:{}", clip.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({"owner": "clip", "id": clip.id})),
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
                    &mut needs_refresh,
                );
            }

            render_effects_section(
                ui,
                project_service,
                history_manager,
                editor_context,
                PropertyOwner::Clip(clip.id),
                &clip.effects,
                local_time,
                fps,
                &mut needs_refresh,
            );

            ui.add_space(12.0);
            ui.heading(format!("Nodes ({})", nodes.len()));
            ui.separator();
            if nodes.is_empty() {
                ui.label("This Clip contains no Nodes.");
            }

            for node in &nodes {
                let is_output = clip.output_node_id == Some(node.id);
                let title = if is_output {
                    format!("{}  ·  Output", node.name)
                } else {
                    node.name.clone()
                };
                egui::CollapsingHeader::new(title)
                    .id_salt(("inspector_clip_node", clip.id, node.id))
                    .default_open(is_output || nodes.len() == 1)
                    .show(ui, |ui| {
                        render_node(
                            ui,
                            node,
                            composition_id,
                            track_id,
                            local_time,
                            fps,
                            project_service,
                            history_manager,
                            editor_context,
                            &mut needs_refresh,
                        );
                    });
            }
        }
        InspectorSelection::Node {
            node,
            track_id,
            containing_clip,
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

fn resolve_selection(project: &Project, selected_id: Uuid) -> Option<InspectorSelection> {
    if let Some(clip) = project.get_clip(selected_id) {
        let nodes = clip
            .node_ids
            .iter()
            .filter_map(|node_id| project.get_node(*node_id).cloned())
            .collect();
        return Some(InspectorSelection::Clip {
            clip: clip.clone(),
            nodes,
            track_id: project.find_track_for_clip(selected_id),
        });
    }

    let node = project.get_node(selected_id)?.clone();
    let containing_clip = project
        .find_parent_clip(selected_id)
        .and_then(|clip_id| project.get_clip(clip_id))
        .cloned();
    Some(InspectorSelection::Node {
        node,
        track_id: project.find_parent_track(selected_id),
        containing_clip,
    })
}

fn render_multi_selection_notice(ui: &mut Ui, editor_context: &EditorContext) {
    let selected_count = editor_context.selection.selected_entities.len();
    if selected_count <= 1 {
        return;
    }
    ui.heading(format!("{selected_count} Items Selected"));
    ui.label(
        egui::RichText::new("(Editing Primary Item)")
            .italics()
            .small(),
    );
    ui.separator();
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
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    needs_refresh: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label("Type:");
        ui.label(node_display_type(node));
    });

    let descriptor_definitions =
        plugin_operation_property_definitions(project_service.get_plugin_manager().as_ref(), node);
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
        inferred_property_definitions(&node.properties, current_time)
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
            &node.properties,
            definitions,
            current_time,
            fps,
            needs_refresh,
        );
    }

    let is_text = matches!(node.content, NodeContent::Generator(GeneratorContent::Text));
    let is_shape = matches!(
        node.content,
        NodeContent::Generator(GeneratorContent::Shape)
    );

    if is_text || is_shape {
        render_styles_section(
            ui,
            project_service,
            history_manager,
            editor_context,
            node.id,
            current_time,
            fps,
            &node.styles,
            needs_refresh,
        );
    }

    if is_text {
        ui.add_space(5.0);
        render_ensemble_section(
            ui,
            project_service,
            history_manager,
            editor_context,
            node.id,
            current_time,
            fps,
            &node.effectors,
            &node.decorators,
            needs_refresh,
        );
    }

    render_effects_section(
        ui,
        project_service,
        history_manager,
        editor_context,
        PropertyOwner::Node(node.id),
        &node.effects,
        current_time,
        fps,
        needs_refresh,
    );
}

fn plugin_operation_property_definitions(
    plugin_manager: &PluginManager,
    node: &Node,
) -> Option<Vec<PropertyDefinition>> {
    let NodeContent::PluginOperation(operation) = &node.content else {
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
    needs_refresh: &mut bool,
) {
    struct Chunk {
        in_grid: bool,
        definitions: Vec<PropertyDefinition>,
    }

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
            qa_scope: qa_owner_scope(owner),
        };
        let actions = if chunk.in_grid {
            let mut actions = Vec::new();
            egui::Grid::new(("inspector_properties", owner, chunk_index))
                .striped(true)
                .show(ui, |ui| {
                    actions = render_property_rows(
                        ui,
                        &chunk.definitions,
                        |name| {
                            properties.get(name).map(|property| {
                                project_service.evaluate_property_value(
                                    property,
                                    properties,
                                    current_time,
                                    fps,
                                )
                            })
                        },
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
                |name| {
                    properties.get(name).map(|property| {
                        project_service.evaluate_property_value(
                            property,
                            properties,
                            current_time,
                            fps,
                        )
                    })
                },
                |name| properties.get(name).cloned(),
                &context,
            )
        };

        let mut context = ActionContext::new(project_service, history_manager, owner, current_time);
        if context.handle_actions(actions, PropertyTarget::Direct, |name| {
            properties.get(name).cloned()
        }) {
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
            let start_definition = Clip::timing_property_definition(CLIP_START_TIME_PROPERTY)
                .expect("Clip start timing metadata");
            let duration_definition = Clip::timing_property_definition(CLIP_DURATION_PROPERTY)
                .expect("Clip duration timing metadata");
            let trim_definition = Clip::timing_property_definition(CLIP_TRIM_IN_PROPERTY)
                .expect("Clip source-start timing metadata");
            let stretch_definition = Clip::timing_property_definition(CLIP_TIME_STRETCH_PROPERTY)
                .expect("Clip stretch timing metadata");
            let start_frame = clip.start_time.into_inner() * fps;
            let duration_frame = clip.duration.into_inner() * fps;
            let trim_in_frame = clip.trim_in.into_inner() * fps;

            ui.label(format!("{} Frame", start_definition.label()));
            let mut edited_start = start_frame;
            let start_config = inspector_timing_drag_config(start_definition, fps, 0.0);
            let response = ui.add(start_config.widget(&mut edited_start));
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
            let duration_config =
                inspector_timing_drag_config(duration_definition, fps, start_frame);
            let response = ui.add(duration_config.widget(&mut edited_end));
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
            let trim_config = inspector_timing_drag_config(trim_definition, fps, 0.0);
            let response = ui.add(trim_config.widget(&mut edited_trim));
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
            let stretch_config = FloatDragValueConfig::from_definition(stretch_definition)
                .expect("Clip stretch has Float drag metadata");
            let response = ui.add(stretch_config.widget(&mut edited_stretch));
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

fn inspector_timing_drag_config(
    definition: &PropertyDefinition,
    fps: f64,
    frame_offset: f64,
) -> FloatDragValueConfig {
    FloatDragValueConfig::from_definition(definition)
        .expect("Clip timing definition has Float drag metadata")
        .transformed(fps, frame_offset, " fr")
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
    match &node.content {
        NodeContent::Media(_) => "Media".to_string(),
        NodeContent::Generator(generator) => match generator {
            GeneratorContent::Shape => "Shape".to_string(),
            GeneratorContent::Text => "Text".to_string(),
            GeneratorContent::Solid => "Solid".to_string(),
            GeneratorContent::SkSL => "SkSL Shader".to_string(),
        },
        NodeContent::Reference(_) => "Reference".to_string(),
        NodeContent::PluginOperation(operation) => format!(
            "Plugin Operation · {} / {}",
            operation.category, operation.operation
        ),
        NodeContent::Merge => "Merge".to_string(),
    }
}

fn inferred_property_definitions(
    properties: &PropertyMap,
    current_time: f64,
) -> Vec<PropertyDefinition> {
    let mut entries: Vec<_> = properties.iter().collect();
    entries.sort_by_key(|(name, _)| *name);
    entries
        .into_iter()
        .filter_map(|(name, property)| {
            let value = property.evaluate_at(current_time);
            let ui_type = match &value {
                PropertyValue::Number(_) => PropertyUiType::Float {
                    min: -1_000_000.0,
                    max: 1_000_000.0,
                    step: 0.1,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                PropertyValue::Integer(_) => PropertyUiType::Integer {
                    min: i64::MIN,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                PropertyValue::String(text) => {
                    if text.contains('\n')
                        || matches!(name.as_str(), "text" | "path" | "shader" | "code")
                    {
                        PropertyUiType::MultilineText
                    } else {
                        PropertyUiType::Text
                    }
                }
                PropertyValue::Boolean(_) => PropertyUiType::Bool,
                PropertyValue::Vec2(_) => PropertyUiType::Vec2 {
                    suffix: String::new(),
                },
                PropertyValue::Vec3(_) => PropertyUiType::Vec3 {
                    suffix: String::new(),
                },
                PropertyValue::Vec4(_) => PropertyUiType::Vec4 {
                    suffix: String::new(),
                },
                PropertyValue::Color(_) => PropertyUiType::Color,
                PropertyValue::Array(_) | PropertyValue::Map(_) => return None,
            };
            Some(PropertyDefinition::new(
                name,
                ui_type,
                &property_label(name),
                value,
            ))
        })
        .collect()
}

fn property_label(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_clip_timing_property(name: &str) -> bool {
    matches!(name, "start_time" | "duration" | "trim_in" | "time_stretch")
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::NodeContainer;
    use library::model::property::Property;

    #[test]
    fn clip_selection_keeps_every_contained_node_in_order() {
        let mut project = Project::new("inspector");
        let first = Node::new("first", NodeContent::Merge);
        let second = Node::new("second", NodeContent::Merge);
        let mut clip = Clip::new("clip", 2.0, 4.0);
        let clip_id = clip.id;
        let first_id = first.id;
        let second_id = second.id;
        clip.node_ids = vec![second_id, first_id];
        clip.output_node_id = Some(first_id);
        project.add_node(first);
        project.add_node(second);
        project.add_clip(clip);

        let Some(InspectorSelection::Clip { nodes, .. }) = resolve_selection(&project, clip_id)
        else {
            panic!("Clip selection should resolve");
        };
        assert_eq!(
            nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![second_id, first_id]
        );
    }

    #[test]
    fn direct_node_selection_stays_node_owned_and_finds_clip_time_scope() {
        let mut project = Project::new("inspector");
        let node = Node::new("leaf", NodeContent::Merge);
        let node_id = node.id;
        let clip = Clip::new("clip", 3.0, 5.0);
        let clip_id = clip.id;
        project.add_node(node);
        project.add_clip(clip);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();

        let Some(InspectorSelection::Node {
            node,
            containing_clip,
            ..
        }) = resolve_selection(&project, node_id)
        else {
            panic!("Node selection should resolve");
        };
        assert_eq!(node.id, node_id);
        assert_eq!(containing_clip.unwrap().id, clip_id);
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

        let inferred = inferred_property_definitions(&node.properties, 0.0);
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
    fn node_and_inspector_timing_adapters_derive_from_the_same_clip_metadata() {
        let duration = Clip::timing_property_definition("duration").unwrap();
        let node = crate::ui::panels::node_editor::node_timing_drag_config(duration);
        let inspector = inspector_timing_drag_config(duration, 30.0, 120.0);

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
        let node_stretch = crate::ui::panels::node_editor::node_timing_drag_config(stretch);
        assert_eq!(node_stretch.hard_min, Some(0.0));
        assert!(stretch
            .validate_value(&PropertyValue::Number(OrderedFloat(0.0)))
            .is_ok());
    }
}
