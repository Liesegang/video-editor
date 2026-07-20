pub mod actions;
pub mod drawing;
pub mod utils;

use actions::*;
pub use utils::PropertyComponent;
use utils::*;

use egui::{Color32, Sense, Ui, Vec2};
use library::model::project::{NodeContainer, Project};
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::EditorService;
use std::sync::{Arc, RwLock};

use crate::action::HistoryManager;
use crate::command::CommandRegistry;
use crate::state::context::EditorContext;
use crate::state::context_types::SelectionTarget;

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
    properties: &'a PropertyMap,
) {
    for (property_key, property) in properties.iter() {
        for component in numeric_components(property) {
            output.push((
                graph_property_name(property_key, component),
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
    let Some(comp_id) = editor_context.active_composition_id else {
        ui.label("No composition selected.");
        return;
    };
    let Some(selected_node_id) = graph_node_selection(editor_context.selection.primary()) else {
        ui.label("Select a Node to edit its keyframes.");
        return;
    };

    let (entity_id, track_id) = {
        let Ok(project) = project.read() else {
            return;
        };
        let Some(track_id) = selected_node_track(&project, selected_node_id, comp_id) else {
            ui.label("Select a Node to edit its keyframes.");
            return;
        };
        (selected_node_id, track_id)
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
        append_property_map(&mut properties_to_plot, entity.properties());

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

fn graph_node_selection(target: Option<SelectionTarget>) -> Option<uuid::Uuid> {
    target.and_then(SelectionTarget::node_id)
}

fn selected_node_track(
    project: &Project,
    node_id: uuid::Uuid,
    comp_id: uuid::Uuid,
) -> Option<uuid::Uuid> {
    project.get_node(node_id)?;
    match project.find_node_container(node_id)? {
        NodeContainer::Composition(id) if id == comp_id => Some(uuid::Uuid::nil()),
        NodeContainer::Track(track_id)
            if project.find_composition_for_track(track_id) == Some(comp_id) =>
        {
            Some(track_id)
        }
        NodeContainer::Clip(clip_id) => project
            .find_track_for_clip(clip_id)
            .filter(|track_id| project.find_composition_for_track(*track_id) == Some(comp_id)),
        NodeContainer::Composition(_) | NodeContainer::Track(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{graph_node_selection, SelectionTarget};
    use uuid::Uuid;

    #[test]
    fn same_uuid_clip_target_is_not_accepted_as_graph_node() {
        let shared_id = Uuid::new_v4();

        assert_eq!(
            graph_node_selection(Some(SelectionTarget::Node(shared_id))),
            Some(shared_id)
        );
        assert_eq!(
            graph_node_selection(Some(SelectionTarget::Clip(shared_id))),
            None
        );
    }
}
