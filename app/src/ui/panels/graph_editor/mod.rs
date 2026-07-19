pub mod actions;
pub mod drawing;
pub mod utils;

use actions::*;
pub use utils::PropertyComponent;
use utils::*;

use egui::{Color32, Sense, Ui, Vec2};
use library::EditorService;
use library::model::project::Project;
use library::model::property::{Property, PropertyMap, PropertyTarget, PropertyValue};
use std::sync::{Arc, RwLock};

use crate::action::HistoryManager;
use crate::command::CommandRegistry;
use crate::state::context::EditorContext;

use crate::command::CommandId;
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};

struct GraphViewportState<'a> {
    pan: &'a mut Vec2,
    zoom_x: &'a mut f32,
    zoom_y: &'a mut f32,
}

impl<'a> ViewportState for GraphViewportState<'a> {
    fn get_pan(&self) -> Vec2 {
        -(*self.pan)
    }
    fn set_pan(&mut self, pan: Vec2) {
        *self.pan = -pan;
    }
    fn get_zoom(&self) -> Vec2 {
        Vec2::new(*self.zoom_x, *self.zoom_y)
    }
    fn set_zoom(&mut self, zoom: Vec2) {
        *self.zoom_x = zoom.x;
        *self.zoom_y = zoom.y;
    }
}

fn numeric_components(property: &Property) -> Vec<PropertyComponent> {
    let value = if property.evaluator == "keyframe" {
        property
            .keyframes()
            .first()
            .map(|keyframe| &keyframe.value)
            .cloned()
    } else if property.evaluator == "constant" {
        property.value().cloned()
    } else {
        None
    };
    match value {
        Some(PropertyValue::Number(_)) => vec![PropertyComponent::Scalar],
        Some(PropertyValue::Vec2(_)) => vec![PropertyComponent::X, PropertyComponent::Y],
        _ => Vec::new(),
    }
}

fn append_property_map<'a>(
    output: &mut Vec<(String, &'a Property, &'a PropertyMap, PropertyComponent)>,
    target: PropertyTarget,
    properties: &'a PropertyMap,
) {
    for (property_key, property) in properties.iter() {
        for component in numeric_components(property) {
            output.push((
                scoped_property_name(target, property_key, component),
                property,
                properties,
                component,
            ));
        }
    }
}

pub fn graph_editor_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    registry: &CommandRegistry,
) {
    let (comp_id, selected_entity_id) = match (
        editor_context.selection.composition_id,
        editor_context.selection.last_selected_entity_id,
    ) {
        (Some(c), Some(e)) => (c, e),
        _ => {
            ui.label("No entity selected.");
            return;
        }
    };

    let (entity_id, track_id) = {
        let Ok(project) = project.read() else {
            return;
        };
        if project.get_node(selected_entity_id).is_none() {
            ui.label("Select a Node to edit its keyframes.");
            return;
        }
        let node_id = selected_entity_id;
        let track_id = project
            .find_parent_track(node_id)
            .or(editor_context.selection.last_selected_track_id)
            .unwrap_or_else(uuid::Uuid::nil);
        (node_id, track_id)
    };
    if editor_context.graph_editor.active_entity_id != Some(entity_id) {
        actions::finish_pending_move(editor_context, project, history_manager);
    }
    if editor_context.graph_editor.begin_entity(entity_id) {
        editor_context.interaction.selected_keyframe = None;
        editor_context.interaction.editing_keyframe = None;
    }

    let mut actions = Vec::new();

    {
        let proj_read = if let Ok(p) = project.read() {
            p
        } else {
            return;
        };

        let composition = if let Some(c) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
            c
        } else {
            return;
        };

        let entity = if let Some(e) = proj_read.get_node(entity_id) {
            e
        } else {
            return;
        };

        let mut properties_to_plot: Vec<(String, &Property, &PropertyMap, PropertyComponent)> =
            Vec::new();

        for (k, p) in entity.properties.iter() {
            let mut components = Vec::new();

            match p.evaluator.as_str() {
                "keyframe" => {
                    // Check first keyframe to determine type
                    if let Some(first) = p.keyframes().first() {
                        match &first.value {
                            PropertyValue::Number(_) => {
                                components.push(PropertyComponent::Scalar);
                            }
                            PropertyValue::Vec2(_) => {
                                components.push(PropertyComponent::X);
                                components.push(PropertyComponent::Y);
                            }
                            _ => {
                                log::trace!(
                                    "GraphEditor: Skipping keyframe property {} with non-numeric type {:?}",
                                    k,
                                    first.value
                                );
                            }
                        }
                    }
                }
                "constant" => match p.value() {
                    Some(PropertyValue::Number(_)) => {
                        components.push(PropertyComponent::Scalar);
                    }
                    Some(PropertyValue::Vec2(_)) => {
                        components.push(PropertyComponent::X);
                        components.push(PropertyComponent::Y);
                    }
                    _ => {
                        log::trace!(
                            "GraphEditor: Skipping constant property {} with non-numeric value {:?}",
                            k,
                            p.value()
                        );
                    }
                },
                _ => {}
            }

            for comp in components {
                properties_to_plot.push((
                    scoped_property_name(PropertyTarget::Direct, k, comp),
                    p,
                    &entity.properties,
                    comp,
                ));
            }
        }

        // Capture clip range for visualization
        let containing_clip = proj_read
            .find_parent_clip(entity.id)
            .and_then(|clip_id| proj_read.get_clip(clip_id));
        let valid_time_range = {
            let start = containing_clip
                .map(|clip| clip.start_time.into_inner())
                .unwrap_or(0.0);
            let duration = containing_clip
                .map(|clip| clip.duration.into_inner())
                .unwrap_or(composition.duration);
            Some((start, start + duration))
        };
        for effect in &entity.effects {
            for (prop_key, prop) in effect.properties.iter() {
                let mut components = Vec::new();
                match prop.evaluator.as_str() {
                    "keyframe" => {
                        if let Some(first) = prop.keyframes().first() {
                            match &first.value {
                                PropertyValue::Number(_) => {
                                    components.push(PropertyComponent::Scalar);
                                }
                                PropertyValue::Vec2(_) => {
                                    components.push(PropertyComponent::X);
                                    components.push(PropertyComponent::Y);
                                }
                                _ => {
                                    log::trace!(
                                        "GraphEditor: Skipping effect property {} with non-numeric type {:?}",
                                        prop_key,
                                        first.value
                                    );
                                }
                            }
                        }
                    }
                    "constant" => match prop.value() {
                        Some(PropertyValue::Number(_)) => {
                            components.push(PropertyComponent::Scalar);
                        }
                        Some(PropertyValue::Vec2(_)) => {
                            components.push(PropertyComponent::X);
                            components.push(PropertyComponent::Y);
                        }
                        _ => {
                            log::trace!(
                                "GraphEditor: Skipping effect property {} with non-numeric value {:?}",
                                prop_key,
                                prop.value()
                            );
                        }
                    },
                    _ => {}
                }

                for comp in components {
                    properties_to_plot.push((
                        scoped_property_name(PropertyTarget::Effect(effect.id), prop_key, comp),
                        prop,
                        &effect.properties,
                        comp,
                    ));
                }
            }
        }

        for style in &entity.styles {
            for (prop_key, prop) in style.properties.iter() {
                let mut components = Vec::new();
                match prop.evaluator.as_str() {
                    "keyframe" => {
                        if let Some(first) = prop.keyframes().first() {
                            match &first.value {
                                PropertyValue::Number(_) => {
                                    components.push(PropertyComponent::Scalar);
                                }
                                PropertyValue::Vec2(_) => {
                                    components.push(PropertyComponent::X);
                                    components.push(PropertyComponent::Y);
                                }
                                _ => {
                                    log::trace!(
                                        "GraphEditor: Skipping style property {} with non-numeric type {:?}",
                                        prop_key,
                                        first.value
                                    );
                                }
                            }
                        }
                    }
                    "constant" => match prop.value() {
                        Some(PropertyValue::Number(_)) => {
                            components.push(PropertyComponent::Scalar);
                        }
                        Some(PropertyValue::Vec2(_)) => {
                            components.push(PropertyComponent::X);
                            components.push(PropertyComponent::Y);
                        }
                        _ => {
                            log::trace!(
                                "GraphEditor: Skipping style property {} with non-numeric value {:?}",
                                prop_key,
                                prop.value()
                            );
                        }
                    },
                    _ => {}
                }

                for comp in components {
                    properties_to_plot.push((
                        scoped_property_name(PropertyTarget::Style(style.id), prop_key, comp),
                        prop,
                        &style.properties,
                        comp,
                    ));
                }
            }
        }

        for effector in &entity.effectors {
            append_property_map(
                &mut properties_to_plot,
                PropertyTarget::Effector(effector.id),
                &effector.properties,
            );
        }

        for decorator in &entity.decorators {
            append_property_map(
                &mut properties_to_plot,
                PropertyTarget::Decorator(decorator.id),
                &decorator.properties,
            );
        }

        if properties_to_plot.is_empty() {
            ui.label("No animatable properties found.");
            return;
        }

        if editor_context.graph_editor.visible_properties.is_empty() {
            for (name, _, _, _) in &properties_to_plot {
                editor_context
                    .graph_editor
                    .visible_properties
                    .insert(name.clone());
            }
        }

        {
            let sidebar_width = 200.0;
            egui::SidePanel::left("graph_sidebar")
                .resizable(true)
                .default_width(sidebar_width)
                .show_inside(ui, |ui| {
                    ui.heading("Properties");
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        const PROPERTY_COLORS: [Color32; 7] = [
                            Color32::RED,
                            Color32::GREEN,
                            Color32::BLUE,
                            Color32::YELLOW,
                            Color32::CYAN,
                            Color32::MAGENTA,
                            Color32::ORANGE,
                        ];

                        for (index, (name, _, _, _)) in properties_to_plot.iter().enumerate() {
                            let color = PROPERTY_COLORS[index % PROPERTY_COLORS.len()];
                            let mut is_visible = editor_context
                                .graph_editor
                                .visible_properties
                                .contains(name);

                            ui.horizontal(|ui| {
                                let (rect, _response) =
                                    ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                ui.painter().circle_filled(rect.center(), 5.0, color);

                                let visibility = ui.checkbox(&mut is_visible, name);
                                crate::qa::register_component_with_metadata(
                                    format!("graph.property_visibility:{name}"),
                                    "graph_property_visibility",
                                    visibility.rect,
                                    visibility.enabled(),
                                    Some(serde_json::json!({
                                        "property": name,
                                        "visible": is_visible,
                                        "entity_id": entity_id,
                                    })),
                                );
                                if visibility.changed() {
                                    if is_visible {
                                        editor_context
                                            .graph_editor
                                            .visible_properties
                                            .insert(name.clone());
                                    } else {
                                        editor_context.graph_editor.visible_properties.remove(name);
                                    }
                                }
                            });
                        }
                    });
                });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                let pixels_per_second = editor_context.graph_editor.zoom_x;
                let pixels_per_unit = editor_context.graph_editor.zoom_y;

                let ruler_height = 24.0;
                let available_rect = ui.available_rect_before_wrap();

                let mut ruler_rect = available_rect;
                ruler_rect.max.y = ruler_rect.min.y + ruler_height;

                let mut graph_rect = available_rect;
                graph_rect.min.y += ruler_height;

                crate::qa::register_component_with_metadata(
                    "graph.canvas",
                    "graph_canvas",
                    graph_rect,
                    true,
                    Some(serde_json::json!({
                        "entity_id": entity_id,
                        "pan": {
                            "x": editor_context.graph_editor.pan.x,
                            "y": editor_context.graph_editor.pan.y,
                        },
                        "zoom_x": pixels_per_second,
                        "zoom_y": pixels_per_unit,
                    })),
                );
                crate::qa::register_component_with_metadata(
                    "graph.ruler",
                    "graph_ruler",
                    ruler_rect,
                    true,
                    Some(serde_json::json!({
                        "entity_id": entity_id,
                        "pixels_per_second": pixels_per_second,
                    })),
                );

                let (_base_response, painter) =
                    ui.allocate_painter(available_rect.size(), Sense::hover());

                let ruler_response =
                    ui.interact(ruler_rect, ui.id().with("ruler"), Sense::click_and_drag());

                let mut state = GraphViewportState {
                    pan: &mut editor_context.graph_editor.pan,
                    zoom_x: &mut editor_context.graph_editor.zoom_x,
                    zoom_y: &mut editor_context.graph_editor.zoom_y,
                };

                let hand_tool_key = registry
                    .commands
                    .iter()
                    .find(|c| c.id == CommandId::HandTool)
                    .and_then(|c| c.shortcut)
                    .map(|(_, k)| k);

                let mut controller =
                    ViewportController::new(ui, ui.id().with("graph"), hand_tool_key).with_config(
                        ViewportConfig {
                            zoom_uniform: false,
                            allow_zoom_x: true,
                            allow_zoom_y: true,
                            ..Default::default()
                        },
                    );

                let (_, graph_response) = controller.interact_with_rect(
                    graph_rect,
                    &mut state,
                    &mut editor_context.interaction.handled_hand_tool_drag,
                );

                let transform = GraphTransform::new(
                    graph_rect,
                    editor_context.graph_editor.pan,
                    pixels_per_second,
                    pixels_per_unit,
                );

                drawing::draw_background(&painter, &transform, ruler_rect, valid_time_range);
                drawing::draw_grid(&painter, &transform, ruler_rect);

                if ruler_response.dragged() || ruler_response.clicked() {
                    if let Some(pos) = ruler_response.interact_pointer_pos() {
                        let (t, _) = transform.screen_to_graph(pos);
                        editor_context.timeline.current_time = t.max(0.0) as f32;
                    }
                }

                let time_mapper =
                    containing_clip.map_or_else(TimeMapper::identity, TimeMapper::from_clip);

                drawing::draw_properties(
                    ui,
                    &painter,
                    &graph_response,
                    &transform,
                    &time_mapper,
                    &properties_to_plot,
                    entity_id,
                    editor_context,
                    project_service,
                    &mut actions,
                    composition.fps,
                );

                drawing::draw_playhead(
                    &painter,
                    &transform,
                    ruler_rect,
                    editor_context.timeline.current_time as f64,
                );
            });
        }
    }

    if editor_context.graph_editor.keyframe_drag.is_some()
        && ui.input(|input| input.pointer.any_released())
        && !actions
            .iter()
            .any(|action| matches!(action, Action::FinishMove))
    {
        actions.push(Action::FinishMove);
    }

    for action in actions {
        actions::process_action(
            action,
            comp_id,
            track_id,
            entity_id,
            project_service,
            project,
            editor_context,
            history_manager,
        );
    }
}
