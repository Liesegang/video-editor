pub mod actions;
pub mod drawing;
pub mod utils;

use actions::*;
pub use utils::PropertyComponent;
use utils::*;

use egui::{Color32, Sense, Ui, Vec2};
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyMap, PropertyValue};
use library::EditorService;
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

pub fn graph_editor_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    registry: &CommandRegistry,
) {
    let (comp_id, track_id, entity_id) = match (
        editor_context.selection.composition_id,
        editor_context.selection.last_selected_track_id,
        editor_context.selection.last_selected_entity_id,
    ) {
        (Some(c), Some(t), Some(e)) => (c, t, e),
        _ => {
            ui.label("No entity selected.");
            return;
        }
    };

    let mut action = Action::None;
    let mut should_push_history = false;

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

        let track = proj_read.get_track(track_id);
        let _ = track; // Not needed directly anymore, using entity from project

        let entity = if let Some(e) = proj_read.get_clip(entity_id) {
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
                                log::trace!("GraphEditor: Skipping keyframe property {} with non-numeric type {:?}", k, first.value);
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
                        log::trace!("GraphEditor: Skipping constant property {} with non-numeric value {:?}", k, p.value());
                    }
                },
                _ => {}
            }

            for comp in components {
                let suffix = match comp {
                    PropertyComponent::Scalar => "",
                    PropertyComponent::X => ".x",
                    PropertyComponent::Y => ".y",
                };
                properties_to_plot.push((format!("{}{}", k, suffix), p, &entity.properties, comp));
            }
        }

        // Capture clip range for visualization
        let (clip_start_frame, clip_end_frame, clip_fps) =
            (entity.in_frame, entity.out_frame, composition.fps);
        let clip_source_begin_frame = entity.source_begin_frame;
        let clip_inherent_fps = entity.fps;

        for (effect_idx, effect) in entity.effects.iter().enumerate() {
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
                                    log::trace!("GraphEditor: Skipping effect property {} with non-numeric type {:?}", prop_key, first.value);
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
                            log::trace!("GraphEditor: Skipping effect property {} with non-numeric value {:?}", prop_key, prop.value());
                        }
                    },
                    _ => {}
                }

                for comp in components {
                    let suffix = match comp {
                        PropertyComponent::Scalar => "",
                        PropertyComponent::X => ".x",
                        PropertyComponent::Y => ".y",
                    };
                    properties_to_plot.push((
                        format!("effect:{}:{}{}", effect_idx, prop_key, suffix),
                        prop,
                        &effect.properties,
                        comp,
                    ));
                }
            }
        }

        for (style_idx, style) in entity.styles.iter().enumerate() {
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
                                    log::trace!("GraphEditor: Skipping style property {} with non-numeric type {:?}", prop_key, first.value);
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
                            log::trace!("GraphEditor: Skipping style property {} with non-numeric value {:?}", prop_key, prop.value());
                        }
                    },
                    _ => {}
                }

                for comp in components {
                    let suffix = match comp {
                        PropertyComponent::Scalar => "",
                        PropertyComponent::X => ".x",
                        PropertyComponent::Y => ".y",
                    };
                    properties_to_plot.push((
                        format!("style:{}:{}{}", style_idx, prop_key, suffix),
                        prop,
                        &style.properties,
                        comp,
                    ));
                }
            }
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
                        let mut color_cycle = [
                            Color32::RED,
                            Color32::GREEN,
                            Color32::BLUE,
                            Color32::YELLOW,
                            Color32::CYAN,
                            Color32::MAGENTA,
                            Color32::ORANGE,
                        ]
                        .iter()
                        .cycle();

                        for (name, _, _, _) in &properties_to_plot {
                            let color = *color_cycle.next().unwrap();
                            let mut is_visible = editor_context
                                .graph_editor
                                .visible_properties
                                .contains(name);

                            ui.horizontal(|ui| {
                                let (rect, _response) =
                                    ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                ui.painter().circle_filled(rect.center(), 5.0, color);

                                if ui.checkbox(&mut is_visible, name).changed() {
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

                let valid_range = if clip_fps > 0.0 {
                    let start_t = clip_start_frame as f64 / clip_fps;
                    let end_t = clip_end_frame as f64 / clip_fps;
                    Some((start_t, end_t))
                } else {
                    None
                };

                drawing::draw_background(&painter, &transform, ruler_rect, valid_range);
                drawing::draw_grid(&painter, &transform, ruler_rect);

                if ruler_response.dragged() || ruler_response.clicked() {
                    if let Some(pos) = ruler_response.interact_pointer_pos() {
                        let (t, _) = transform.from_screen(pos);
                        editor_context.timeline.current_time = t.max(0.0) as f32;
                    }
                }

                let time_mapper = TimeMapper {
                    clip_start_frame: clip_start_frame as i64,
                    clip_source_begin_frame,
                    clip_fps,
                    clip_inherent_fps,
                };

                drawing::draw_properties(
                    ui,
                    &painter,
                    &graph_response,
                    &transform,
                    &time_mapper,
                    &properties_to_plot,
                    editor_context,
                    project_service,
                    &mut action,
                    &mut should_push_history,
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
