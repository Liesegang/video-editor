use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, UiKind, Vec2};
use library::animation::EasingFunction;
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyMap, PropertyValue};
use library::EditorService;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyComponent {
    Scalar,
    X,
    Y,
}

use crate::action::HistoryManager;
use crate::command::CommandRegistry;
use crate::state::context::EditorContext;

use crate::command::CommandId;
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};

enum Action {
    Select(String, usize),
    Move(String, usize, f64, f64), // prop_key, index, new_time, new_value
    Add(String, f64, f64),         // prop_key, time, value
    SetEasing(String, usize, EasingFunction),
    Remove(String, usize),
    EditKeyframe(String, usize),
    None,
}

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

        let track = if let Some(t) = composition.tracks.iter().find(|t| t.id == track_id) {
            t
        } else {
            return;
        };

        let entity = if let Some(e) = track.clips.iter().find(|c| c.id == entity_id) {
            e
        } else {
            return;
        };

        let mut properties_to_plot: Vec<(String, &Property, &PropertyMap, PropertyComponent)> =
            Vec::new();

        for (k, p) in entity.properties.iter() {
            let mut include = false;
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
                            _ => {}
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
                    _ => {}
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
                    format!("{}{}", k, suffix),
                    p,
                    &entity.properties,
                    comp,
                ));
            }
        }

        // Capture clip range for visualization
        let (clip_start_frame, clip_end_frame, clip_fps) =
            (entity.in_frame, entity.out_frame, composition.fps);

        // Add Effect Properties

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
                                _ => {}
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
                        _ => {}
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

        // Add Style Properties
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
                                _ => {}
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
                        _ => {}
                    },
                    _ => {}
                }

                for comp in components {
                    let suffix = match comp {
                        PropertyComponent::Scalar => "",
                        PropertyComponent::X => ".x",
                        PropertyComponent::Y => ".y",
                    };
                    // Use a recognizable prefix, style:idx:key
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

        // Initialize visible_properties if it's empty (first load)
        if editor_context.graph_editor.visible_properties.is_empty() {
            for (name, _, _, _) in &properties_to_plot {
                editor_context
                    .graph_editor
                    .visible_properties
                    .insert(name.clone());
            }
        }

        // Layout: Sidebar (List) + Graph
        // Layout: Sidebar (List) + Graph
        // ui.horizontal removed to fix height issue
        {
            // --- Sidebar: Property List ---
            let sidebar_width = 200.0;
            egui::SidePanel::left("graph_sidebar")
                .resizable(true)
                .default_width(sidebar_width)
                .show_inside(ui, |ui| {
                    ui.heading("Properties");
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // We need a stable iterator for colors to match the graph ones
                        // The existing logic used a cycle iterator *during* the loop.
                        // We should probably generate colors deterministically or consistent with the loop order.
                        // Since properties_to_plot is filtered deterministically, index-based coloring is fine.

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
                                // Color indicator
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

            // --- Main Graph Area ---
            egui::CentralPanel::default().show_inside(ui, |ui| {
                // Existing Graph Logic... (wrapped)

                // Note: We need to filter properties_to_plot for the graph loop based on visibility.
                // But the rest of the logic (time cursor, zoom, etc.) remains.
                let pixels_per_second = editor_context.graph_editor.zoom_x;
                let pixels_per_unit = editor_context.graph_editor.zoom_y;

                // Layout: Top Ruler + Main Graph
                let ruler_height = 24.0;
                let available_rect = ui.available_rect_before_wrap();

                // Ruler Rect
                let mut ruler_rect = available_rect;
                ruler_rect.max.y = ruler_rect.min.y + ruler_height;

                // Graph Rect
                let mut graph_rect = available_rect;
                graph_rect.min.y += ruler_height;

                // Allocate Graph Area (Covers everything including ruler)
                // We use Sense::hover() here so the painter allocation doesn't steal inputs from manual interact calls below.
                let (_base_response, painter) =
                    ui.allocate_painter(available_rect.size(), Sense::hover());

                // Explicitly handle interactions for disjoint areas
                let ruler_response =
                    ui.interact(ruler_rect, ui.id().with("ruler"), Sense::click_and_drag());

                // Viewport Controller Logic
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

                let (_changed, graph_response) = controller.interact_with_rect(
                    graph_rect,
                    &mut state,
                    &mut editor_context.interaction.handled_hand_tool_drag,
                );

                // Interaction Handling

                // Ruler Interaction (Seek)
                if ruler_response.dragged() || ruler_response.clicked() {
                    if let Some(pos) = ruler_response.interact_pointer_pos() {
                        let x = pos.x;
                        let time = (x - graph_rect.min.x - editor_context.graph_editor.pan.x)
                            / pixels_per_second;
                        editor_context.timeline.current_time = time.max(0.0) as f32;
                    }
                }

                // Graph Interaction (Pan)

                // We use graph_response for Keyframe/Curve interactions in the loop as well?
                // The keyframe loop uses `ui.interact(point_rect, ...)` which is fine (on top).
                // But "Double Click to Add" needs to check graph_response.

                // Note: We need to update checks later in the file that used `response`.
                // I will use `graph_response` for those generic graph interactions.
                let response = graph_response.clone(); // Alias for compatibility with existing code lower down?
                                                       // Actually, I should update the usages below.
                                                       // But to minimize diff, let's see.
                                                       // Usages below:
                                                       // point_response context menu: independent.
                                                       // response.double_clicked(): used for adding keyframe.
                                                       // response.id.with(...): used for IDs.

                // So aliasing `response` to `graph_response` (or `base_response`?)
                // `base_response` covers all, but `graph_response` is where we want "add keyframe" to happen (not on ruler).
                // So `let response = graph_response;` seems appropriate.

                let _response = graph_response; // Shadowing the tuple variable if I extracted it? No, `response` from allocate_painter.
                                                // Wait, allocate_painter returned `response`. I named it `base_response`.
                                                // So I can just define `let response = graph_response;` here.

                // Paint Background (Invalid Area - Lighter)
                painter.rect_filled(graph_rect, 0.0, Color32::from_gray(45));

                // Highlight Clip Valid Range (Valid Area - Darker)
                if clip_fps > 0.0 {
                    let start_t = clip_start_frame as f64 / clip_fps;
                    let end_t = clip_end_frame as f64 / clip_fps;

                    let start_x = graph_rect.min.x
                        + editor_context.graph_editor.pan.x
                        + (start_t as f32 * pixels_per_second);
                    let end_x = graph_rect.min.x
                        + editor_context.graph_editor.pan.x
                        + (end_t as f32 * pixels_per_second);

                    // Clamp to graph area to avoid drawing over ruler if logic was different,
                    // but pure x coordinates are fine with clip_rect.
                    // We want to highlight the *valid* area.

                    let highlight_rect = Rect::from_min_max(
                        Pos2::new(start_x.max(graph_rect.min.x), graph_rect.min.y),
                        Pos2::new(end_x.min(graph_rect.max.x), graph_rect.max.y),
                    );

                    if highlight_rect.is_positive() {
                        painter.rect_filled(highlight_rect, 0.0, Color32::from_gray(25));
                    }
                }

                painter.rect_filled(ruler_rect, 0.0, Color32::from_gray(40));
                painter.line_segment(
                    [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
                    Stroke::new(1.0, Color32::BLACK),
                );

                let to_screen_pos = |time: f64, value: f64| -> Pos2 {
                    let x = graph_rect.min.x
                        + editor_context.graph_editor.pan.x
                        + (time as f32 * pixels_per_second);
                    let zero_y = graph_rect.center().y + editor_context.graph_editor.pan.y;
                    let y = zero_y - (value as f32 * pixels_per_unit);
                    Pos2::new(x, y)
                };

                let from_screen_pos = |pos: Pos2| -> (f64, f64) {
                    let x = pos.x;
                    let time = (x - graph_rect.min.x - editor_context.graph_editor.pan.x)
                        / pixels_per_second;

                    let zero_y = graph_rect.center().y + editor_context.graph_editor.pan.y;
                    let y = pos.y;
                    let value = (zero_y - y) / pixels_per_unit;
                    (time as f64, value as f64)
                };

                // --- Draw Rulers and Grids ---

                // Time Grid & Ruler
                let start_time = (-editor_context.graph_editor.pan.x / pixels_per_second) as f64;
                let end_time = ((graph_rect.width() - editor_context.graph_editor.pan.x)
                    / pixels_per_second) as f64;

                // Adaptive step size
                let min_step_px = 50.0;
                let step_time = (min_step_px / pixels_per_second).max(0.01);
                // Snap to nice numbers (1, 0.5, 0.1 etc)
                let step_power = step_time.log10().floor();
                let step_base = 10.0f32.powf(step_power);
                let step_time = if step_time / step_base < 2.0 {
                    step_base
                } else if step_time / step_base < 5.0 {
                    step_base * 2.0
                } else {
                    step_base * 5.0
                };

                let start_step = (start_time / step_time as f64).floor() as i64;
                let end_step = (end_time / step_time as f64).ceil() as i64;

                for i in start_step..=end_step {
                    let t = i as f64 * step_time as f64;
                    let x = graph_rect.min.x
                        + editor_context.graph_editor.pan.x
                        + (t as f32 * pixels_per_second);

                    if x >= graph_rect.min.x && x <= graph_rect.max.x {
                        // Main Vertical Line
                        painter.line_segment(
                            [
                                Pos2::new(x, graph_rect.min.y),
                                Pos2::new(x, graph_rect.max.y),
                            ],
                            Stroke::new(1.0, Color32::from_gray(40)),
                        );

                        // Ruler Tick & Label
                        painter.line_segment(
                            [
                                Pos2::new(x, ruler_rect.max.y),
                                Pos2::new(x, ruler_rect.max.y - 10.0),
                            ],
                            Stroke::new(1.0, Color32::GRAY),
                        );
                        painter.text(
                            Pos2::new(x + 2.0, ruler_rect.min.y + 2.0),
                            egui::Align2::LEFT_TOP,
                            format!("{:.2}", t),
                            egui::FontId::proportional(10.0),
                            Color32::GRAY,
                        );
                    }
                }

                // Value Grid & Left Axis
                // Calculate visible value range
                // zero_y = center + pan.y
                // y = zero_y - val * zoom
                // val = (zero_y - y) / zoom
                let zero_y = graph_rect.center().y + editor_context.graph_editor.pan.y;
                let min_val = (zero_y - graph_rect.max.y) / pixels_per_unit;
                let max_val = (zero_y - graph_rect.min.y) / pixels_per_unit;

                let v_step_val = (30.0 / pixels_per_unit).max(0.01); // 30px min spacing
                let v_step_power = v_step_val.log10().floor();
                let v_step_base = 10.0f32.powf(v_step_power);
                let v_step_val = if v_step_val / v_step_base < 2.0 {
                    v_step_base
                } else if v_step_val / v_step_base < 5.0 {
                    v_step_base * 2.0
                } else {
                    v_step_base * 5.0
                };

                let start_v = (min_val / v_step_val).floor() as i64;
                let end_v = (max_val / v_step_val).ceil() as i64;

                for i in start_v..=end_v {
                    let val = i as f32 * v_step_val;
                    let y = zero_y - (val * pixels_per_unit);

                    if y >= graph_rect.min.y && y <= graph_rect.max.y {
                        // Horizontal Line
                        let color = if i == 0 {
                            Color32::from_gray(80)
                        } else {
                            Color32::from_gray(40)
                        };
                        painter.line_segment(
                            [
                                Pos2::new(graph_rect.min.x, y),
                                Pos2::new(graph_rect.max.x, y),
                            ],
                            Stroke::new(1.0, color),
                        );
                        painter.text(
                            Pos2::new(graph_rect.min.x + 2.0, y - 2.0),
                            egui::Align2::LEFT_BOTTOM,
                            format!("{:.2}", val),
                            egui::FontId::proportional(10.0),
                            Color32::from_gray(150),
                        );
                    }
                }
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

                for (name, property, map, component) in properties_to_plot {
                    let color = *color_cycle.next().unwrap();

                    if !editor_context
                        .graph_editor
                        .visible_properties
                        .contains(&name)
                    {
                        continue;
                    }

                    match property.evaluator.as_str() {
                        "constant" => {
                            let maybe_val = match component {
                                PropertyComponent::Scalar => {
                                    property.value().and_then(|v| v.get_as::<f64>())
                                }
                                PropertyComponent::X => property.value().and_then(|v| {
                                    if let PropertyValue::Vec2(vec) = v {
                                        Some(vec.x.into_inner())
                                    } else {
                                        None
                                    }
                                }),
                                PropertyComponent::Y => property.value().and_then(|v| {
                                    if let PropertyValue::Vec2(vec) = v {
                                        Some(vec.y.into_inner())
                                    } else {
                                        None
                                    }
                                }),
                            };
                            if let Some(val) = maybe_val {
                                let y = to_screen_pos(0.0, val).y;
                                if y >= graph_rect.min.y && y <= graph_rect.max.y {
                                    painter.line_segment(
                                        [
                                            Pos2::new(graph_rect.min.x, y),
                                            Pos2::new(graph_rect.max.x, y),
                                        ],
                                        Stroke::new(2.0, color),
                                    );
                                    painter.text(
                                        Pos2::new(graph_rect.min.x + 40.0, y - 5.0),
                                        egui::Align2::LEFT_BOTTOM,
                                        format!("{}: {:.2}", name, val),
                                        egui::FontId::default(),
                                        color,
                                    );

                                    // Double Click to add keyframe logic
                                    if response.double_clicked() {
                                        if let Some(pointer_pos) = response.interact_pointer_pos() {
                                            if (pointer_pos.y - y).abs() < 5.0
                                                && graph_rect.contains(pointer_pos)
                                            {
                                                let (t, _) = from_screen_pos(pointer_pos);
                                                if let Action::None = action {
                                                    action =
                                                        Action::Add(name.clone(), t.max(0.0), val);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "keyframe" | "expression" => {
                            // Use sampling based rendering for consistent visualization (Keyframes, Expressions, etc.)

                            // 1. Draw Curve via Sampling
                            let mut path_points = Vec::new();
                            let step_px = 2.0f32; // Sample every 2 pixels

                            let start_x = graph_rect.min.x;
                            let end_x = graph_rect.max.x;
                            let steps = ((end_x - start_x) / step_px).ceil() as usize;

                            for s in 0..=steps {
                                let x = start_x + s as f32 * step_px;
                                // Convert screen x to time
                                // x = min.x + pan.x + (time * pps)
                                // time = (x - min.x - pan.x) / pps
                                let time =
                                    (x - graph_rect.min.x - editor_context.graph_editor.pan.x)
                                        / pixels_per_second;

                                // Evaluate
                                let value_pv = project_service.evaluate_property_value(
                                    property,
                                    map,
                                    time as f64,
                                    composition.fps,
                                );
                                let val_f64 = match component {
                                    PropertyComponent::Scalar => value_pv.get_as::<f64>(),
                                    PropertyComponent::X => {
                                        if let PropertyValue::Vec2(vec) = value_pv {
                                            Some(vec.x.into_inner())
                                        } else {
                                            None
                                        }
                                    }
                                    PropertyComponent::Y => {
                                        if let PropertyValue::Vec2(vec) = value_pv {
                                            Some(vec.y.into_inner())
                                        } else {
                                            None
                                        }
                                    }
                                };

                                if let Some(val) = val_f64 {
                                    let pos = to_screen_pos(time as f64, val);
                                    // Clamp Y to reasonable bounds to avoid drawing issues far off screen?
                                    // Painter usually handles it, but let's be safe if needed.
                                    // Actually egui painter clips to clip_rect, so it's fine.
                                    path_points.push(pos);
                                }
                            }

                            if path_points.len() > 1 {
                                painter
                                    .add(egui::Shape::line(path_points, Stroke::new(2.0, color)));
                            }

                            // 2. Draw Keyframe Dots (Overlay) if it is a keyframe property
                            if property.evaluator == "keyframe" {
                                let keyframes = property.keyframes();
                                let mut sorted_kf = keyframes.clone();
                                sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));

                                for (i, kf) in sorted_kf.iter().enumerate() {
                                    let t = kf.time.into_inner();
                                    let val_f64 = match component {
                                        PropertyComponent::Scalar => kf.value.get_as::<f64>(),
                                        PropertyComponent::X => {
                                            if let PropertyValue::Vec2(vec) = &kf.value {
                                                Some(vec.x.into_inner())
                                            } else {
                                                None
                                            }
                                        }
                                        PropertyComponent::Y => {
                                            if let PropertyValue::Vec2(vec) = &kf.value {
                                                Some(vec.y.into_inner())
                                            } else {
                                                None
                                            }
                                        }
                                    };
                                    let val = val_f64.unwrap_or(0.0);
                                    let kf_pos = to_screen_pos(t, val);

                                    // Skip if out of view (optimization)
                                    if !graph_rect.expand(10.0).contains(kf_pos) {
                                        continue;
                                    }

                                    // Interaction area
                                    let point_rect =
                                        Rect::from_center_size(kf_pos, Vec2::splat(12.0));
                                    let point_id = response.id.with(&name).with(i);
                                    let point_response =
                                        ui.interact(point_rect, point_id, Sense::click_and_drag());

                                    let is_selected = editor_context
                                        .interaction
                                        .selected_keyframe
                                        .as_ref()
                                        .map_or(false, |(s_name, s_idx)| {
                                            s_name == &name && *s_idx == i
                                        });

                                    // Draw Dot
                                    let dot_color =
                                        if is_selected { Color32::WHITE } else { color };
                                    let radius = if is_selected { 6.0 } else { 4.0 };
                                    painter.circle_filled(kf_pos, radius, dot_color);

                                    // Selection
                                    if point_response.clicked() {
                                        action = Action::Select(name.clone(), i);
                                    }

                                    // History: drag stopped
                                    if point_response.drag_stopped() {
                                        should_push_history = true;
                                    }

                                    // Context Menu
                                    let name_for_menu = name.clone();
                                    point_response.context_menu(|ui| {
                                        ui.label(format!("Keyframe {} - {}", i, name_for_menu));
                                        ui.separator();
                                        let mut chosen_easing = None;
                                        crate::ui::easing_menus::show_easing_menu(
                                            ui,
                                            None,
                                            |easing| {
                                                chosen_easing = Some(easing);
                                            },
                                        );

                                        if let Some(easing) = chosen_easing {
                                            action =
                                                Action::SetEasing(name_for_menu.clone(), i, easing);
                                            should_push_history = true;
                                            ui.close_kind(UiKind::Menu);
                                        }

                                        ui.separator();
                                        if ui.button("Edit Keyframe...").clicked() {
                                            action = Action::EditKeyframe(name_for_menu.clone(), i);
                                            ui.close_kind(UiKind::Menu);
                                        }

                                        ui.separator();
                                        if ui
                                            .button(
                                                egui::RichText::new("Delete Keyframe")
                                                    .color(Color32::RED),
                                            )
                                            .clicked()
                                        {
                                            action = Action::Remove(name_for_menu.clone(), i);
                                            should_push_history = true;
                                            ui.close_kind(UiKind::Menu);
                                        }
                                    });

                                    // Dragging
                                    if is_selected && point_response.dragged() {
                                        let (new_t, new_val) =
                                            from_screen_pos(kf_pos + point_response.drag_delta());
                                        action =
                                            Action::Move(name.clone(), i, new_t.max(0.0), new_val);
                                    }
                                }

                                // Add Keyframe (Double Click) logic constraint
                                // Re-use logic but adapted?
                                // The previous logic used 'closest point on curve segment'.
                                // Now we effectively have the curve implicitly.
                                // We can check distance to sampled points?
                                // Or keep the old logic just for hit testing?
                                // Actually, for adding keyframes, checking strictly against the curve is nice.
                                // But 'evaluate_property_value(t)' effectively gives us the Y for any T.
                                // So we can check if click_y is close to eval(click_t).

                                if response.double_clicked() {
                                    if let Some(pointer_pos) = response.interact_pointer_pos() {
                                        if graph_rect.contains(pointer_pos) {
                                            let (t, _) = from_screen_pos(pointer_pos);

                                            // Evaluate at pointer time
                                            let value_pv = project_service
                                                .evaluate_property_value(property, map, t, composition.fps);
                                            let val_at_t = match component {
                                                PropertyComponent::Scalar => {
                                                    value_pv.get_as::<f64>().unwrap_or(0.0)
                                                }
                                                PropertyComponent::X => {
                                                    if let PropertyValue::Vec2(vec) = value_pv {
                                                        vec.x.into_inner()
                                                    } else {
                                                        0.0
                                                    }
                                                }
                                                PropertyComponent::Y => {
                                                    if let PropertyValue::Vec2(vec) = value_pv {
                                                        vec.y.into_inner()
                                                    } else {
                                                        0.0
                                                    }
                                                }
                                            };
                                            let curve_pos = to_screen_pos(t, val_at_t);

                                            // Distance check
                                            if (pointer_pos.y - curve_pos.y).abs() < 10.0 {
                                                if let Action::None = action {
                                                    action = Action::Add(
                                                        name.clone(),
                                                        t.max(0.0),
                                                        val_at_t,
                                                    );
                                                    should_push_history = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        _ => {}
                    }
                }

                // Draw Playhead in Graph
                let t_cursor = editor_context.timeline.current_time as f64;
                let x_cursor = graph_rect.min.x
                    + editor_context.graph_editor.pan.x
                    + (t_cursor as f32 * pixels_per_second);
                if x_cursor >= graph_rect.min.x && x_cursor <= graph_rect.max.x {
                    painter.line_segment(
                        [
                            Pos2::new(x_cursor, graph_rect.min.y),
                            Pos2::new(x_cursor, graph_rect.max.y),
                        ],
                        Stroke::new(2.0, Color32::RED),
                    );
                }
                // Draw Playhead in Ruler
                let t_cursor = editor_context.timeline.current_time as f64;
                let x_cursor = ruler_rect.min.x
                    + editor_context.graph_editor.pan.x
                    + (t_cursor as f32 * pixels_per_second);

                if x_cursor >= ruler_rect.min.x && x_cursor <= ruler_rect.max.x {
                    // ... existing drawing for ruler playhead or use shared cursor logic ..
                    // Actually lines 604-611 seem to draw the triangle in ruler.
                    // I'll leave it as is, just closing the CentralPanel and Horizontal block.
                }

                if x_cursor >= ruler_rect.min.x && x_cursor <= ruler_rect.max.x {
                    // Triangle head
                    let head_size = 6.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            Pos2::new(x_cursor, ruler_rect.max.y),
                            Pos2::new(x_cursor - head_size, ruler_rect.max.y - head_size),
                            Pos2::new(x_cursor + head_size, ruler_rect.max.y - head_size),
                        ],
                        Color32::RED,
                        Stroke::NONE,
                    ));
                }
            }); // End CentralPanel (Graph)
        } // End Scope
    } // End Read Lock Scope

    // Process Actions
    // Helper to parse key
    let parse_key = |key: &str| -> Option<(usize, String)> {
        if key.starts_with("effect:") {
            let parts: Vec<&str> = key.splitn(3, ':').collect();
            if parts.len() == 3 {
                if let Ok(idx) = parts[1].parse::<usize>() {
                    return Some((idx, parts[2].to_string()));
                }
            }
        }
        None
    };

    let parse_style_key = |key: &str| -> Option<(usize, String)> {
        if key.starts_with("style:") {
            let parts: Vec<&str> = key.splitn(3, ':').collect();
            if parts.len() == 3 {
                if let Ok(idx) = parts[1].parse::<usize>() {
                    return Some((idx, parts[2].to_string()));
                }
            }
        }
        None
    };

    match action {
        Action::Select(name, idx) => {
            editor_context.interaction.selected_keyframe = Some((name, idx));
        }
        Action::Move(name, idx, new_time, new_val) => {
            let (base_name, suffix) = if name.ends_with(".x") {
                (name.trim_end_matches(".x"), Some(PropertyComponent::X))
            } else if name.ends_with(".y") {
                (name.trim_end_matches(".y"), Some(PropertyComponent::Y))
            } else {
                (name.as_str(), None)
            };

            if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                // Effect property
                let mut current_pv = None;
                if let Ok(proj) = project.read() {
                    // Navigate to find keyframe
                    if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                        if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                            if let Some(clip) = track.clips.iter().find(|c| c.id == entity_id) {
                                if let Some(effect) = clip.effects.get(eff_idx) {
                                    if let Some(prop) = effect.properties.get(&prop_key) {
                                        let keyframes = prop.keyframes();
                                        let mut sorted_kf = keyframes.clone();
                                        sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                                        if let Some(kf) = sorted_kf.get(idx) {
                                            current_pv = Some(kf.value.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                    match suffix {
                        Some(PropertyComponent::X) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: OrderedFloat(new_val),
                            y: old_vec.y,
                        }),
                        Some(PropertyComponent::Y) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: old_vec.x,
                            y: OrderedFloat(new_val),
                        }),
                        _ => PropertyValue::Number(OrderedFloat(new_val)),
                    }
                } else {
                    PropertyValue::Number(OrderedFloat(new_val))
                };

                let _ = project_service.update_effect_keyframe_by_index(
                    comp_id,
                    track_id,
                    entity_id,
                    eff_idx,
                    &prop_key,
                    idx,
                    Some(new_time),
                    Some(new_pv),
                    None,
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                // Style property
                let mut current_pv = None;
                if let Ok(proj) = project.read() {
                    if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                        if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                            if let Some(clip) = track.clips.iter().find(|c| c.id == entity_id) {
                                if let Some(style) = clip.styles.get(style_idx) {
                                    if let Some(prop) = style.properties.get(&prop_key) {
                                        let keyframes = prop.keyframes();
                                        let mut sorted_kf = keyframes.clone();
                                        sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                                        if let Some(kf) = sorted_kf.get(idx) {
                                            current_pv = Some(kf.value.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                    match suffix {
                        Some(PropertyComponent::X) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: OrderedFloat(new_val),
                            y: old_vec.y,
                        }),
                        Some(PropertyComponent::Y) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: old_vec.x,
                            y: OrderedFloat(new_val),
                        }),
                        _ => PropertyValue::Number(OrderedFloat(new_val)),
                    }
                } else {
                    PropertyValue::Number(OrderedFloat(new_val))
                };

                let _ = project_service.update_style_keyframe_by_index(
                    comp_id,
                    track_id,
                    entity_id,
                    style_idx,
                    &prop_key,
                    idx,
                    Some(new_time),
                    Some(new_pv),
                    None,
                );
            } else {
                // Clip property
                let mut current_pv = None;
                if let Ok(proj) = project.read() {
                    if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                        if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                            if let Some(clip) = track.clips.iter().find(|c| c.id == entity_id) {
                                if let Some(prop) = clip.properties.get(base_name) {
                                     let keyframes = prop.keyframes();
                                     let mut sorted_kf = keyframes.clone();
                                     sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                                     if let Some(kf) = sorted_kf.get(idx) {
                                         current_pv = Some(kf.value.clone());
                                     }
                                }
                            }
                        }
                    }
                }

                let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                    match suffix {
                        Some(PropertyComponent::X) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: OrderedFloat(new_val),
                            y: old_vec.y,
                        }),
                        Some(PropertyComponent::Y) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: old_vec.x,
                            y: OrderedFloat(new_val),
                        }),
                        _ => PropertyValue::Number(OrderedFloat(new_val)),
                    }
                } else {
                    PropertyValue::Number(OrderedFloat(new_val))
                };

                let _ = project_service.update_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    base_name,
                    idx,
                    Some(new_time),
                    Some(new_pv),
                    None,
                );
            }
        }
        Action::Add(name, time, val) => {
             let (base_name, suffix) = if name.ends_with(".x") {
                (name.trim_end_matches(".x"), Some(PropertyComponent::X))
            } else if name.ends_with(".y") {
                (name.trim_end_matches(".y"), Some(PropertyComponent::Y))
            } else {
                (name.as_str(), None)
            };

             let mut current_val_at_t = None;
             if let Ok(proj) = project.read() {
                   if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                        if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                            if let Some(entity) = track.clips.iter().find(|c| c.id == entity_id) {
                                if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                                    if let Some(effect) = entity.effects.get(eff_idx) {
                                        if let Some(prop) = effect.properties.get(&prop_key) {
                                            current_val_at_t = Some(project_service.evaluate_property_value(prop, &effect.properties, time, comp.fps));
                                        }
                                    }
                                } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                                     // Style Property
                                     if let Some(style) = entity.styles.get(style_idx) {
                                         if let Some(prop) = style.properties.get(&prop_key) {
                                              current_val_at_t = Some(project_service.evaluate_property_value(prop, &style.properties, time, comp.fps));
                                         }
                                     }
                                } else {
                                     if let Some(prop) = entity.properties.get(base_name) {
                                          current_val_at_t = Some(project_service.evaluate_property_value(prop, &entity.properties, time, comp.fps));
                                     }
                                }
                            }
                        }
                   }
             }

             let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_val_at_t {
                  match suffix {
                        Some(PropertyComponent::X) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: OrderedFloat(val),
                            y: old_vec.y,
                        }),
                        Some(PropertyComponent::Y) => PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: old_vec.x,
                            y: OrderedFloat(val),
                        }),
                        _ => PropertyValue::Number(OrderedFloat(val)),
                    }
             } else {
                  PropertyValue::Number(OrderedFloat(val))
             };

            if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                let _ = project_service.add_effect_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    eff_idx,
                    &prop_key,
                    time,
                    new_pv,
                    None,
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                 let _ = project_service.add_style_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    style_idx,
                    &prop_key,
                    time,
                    new_pv,
                    None,
                );
            } else {
                 let _ = project_service.add_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    base_name,
                    time,
                    new_pv,
                    None,
                );
            }
        }
        Action::SetEasing(name, idx, easing) => {
            if let Some((eff_idx, prop_key)) = parse_key(&name) {
                let _ = project_service.update_effect_keyframe_by_index(
                    comp_id,
                    track_id,
                    entity_id,
                    eff_idx,
                    &prop_key,
                    idx,
                    None,
                    None,
                    Some(easing),
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(&name) {
                let _ = project_service.update_style_keyframe_by_index(
                    comp_id,
                    track_id,
                    entity_id,
                    style_idx,
                    &prop_key,
                    idx,
                    None,
                    None,
                    Some(easing),
                );
            } else {
                let _ = project_service.update_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    &name,
                    idx,
                    None, // Keep existing time
                    None, // Keep existing value
                    Some(easing),
                );
            }
        }
        Action::Remove(name, idx) => {
            if let Some((eff_idx, prop_key)) = parse_key(&name) {
                let _ = project_service.remove_effect_keyframe_by_index(
                    comp_id, track_id, entity_id, eff_idx, &prop_key, idx,
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(&name) {
                let _ = project_service.remove_style_keyframe(
                    comp_id, track_id, entity_id, style_idx, &prop_key, idx,
                );
            } else {
                let _ = project_service.remove_keyframe(comp_id, track_id, entity_id, &name, idx);
            }
        }
        Action::EditKeyframe(name, idx) => {
            if let Ok(project) = project.read() {
                if let Some(comp) = project.compositions.iter().find(|c| c.id == comp_id) {
                    if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                        if let Some(clip) = track.clips.iter().find(|c| c.id == entity_id) {
                            // Effect Property
                            if let Some((eff_idx, prop_key)) = parse_key(&name) {
                                if let Some(effect) = clip.effects.get(eff_idx) {
                                    if let Some(prop) = effect.properties.get(&prop_key) {
                                        if prop.evaluator == "keyframe" {
                                            let keyframes = prop.keyframes();
                                            if let Some(kf) = keyframes.get(idx) {
                                                editor_context.keyframe_dialog.is_open = true;
                                                editor_context.keyframe_dialog.track_id =
                                                    Some(track_id);
                                                editor_context.keyframe_dialog.entity_id =
                                                    Some(entity_id);
                                                editor_context.keyframe_dialog.property_name =
                                                    name.clone();
                                                editor_context.keyframe_dialog.keyframe_index = idx;
                                                editor_context.keyframe_dialog.time =
                                                    kf.time.into_inner();
                                                editor_context.keyframe_dialog.value =
                                                    kf.value.get_as::<f64>().unwrap_or(0.0);
                                                editor_context.keyframe_dialog.easing =
                                                    kf.easing.clone();
                                            }
                                        }
                                    }
                                }
                            }
                            // Style Property
                            else if let Some((style_idx, prop_key)) = parse_style_key(&name) {
                                if let Some(style) = clip.styles.get(style_idx) {
                                    if let Some(prop) = style.properties.get(&prop_key) {
                                        if prop.evaluator == "keyframe" {
                                            let keyframes = prop.keyframes();
                                            if let Some(kf) = keyframes.get(idx) {
                                                editor_context.keyframe_dialog.is_open = true;
                                                editor_context.keyframe_dialog.track_id =
                                                    Some(track_id);
                                                editor_context.keyframe_dialog.entity_id =
                                                    Some(entity_id);
                                                editor_context.keyframe_dialog.property_name =
                                                    name.clone();
                                                editor_context.keyframe_dialog.keyframe_index = idx;
                                                editor_context.keyframe_dialog.time =
                                                    kf.time.into_inner();
                                                editor_context.keyframe_dialog.value =
                                                    kf.value.get_as::<f64>().unwrap_or(0.0);
                                                editor_context.keyframe_dialog.easing =
                                                    kf.easing.clone();
                                            }
                                        }
                                    }
                                }
                            }
                            // Clip Property
                            else if let Some(prop) = clip.properties.get(&name) {
                                if prop.evaluator == "keyframe" {
                                    let keyframes = prop.keyframes();
                                    if let Some(kf) = keyframes.get(idx) {
                                        editor_context.keyframe_dialog.is_open = true;
                                        editor_context.keyframe_dialog.track_id = Some(track_id);
                                        editor_context.keyframe_dialog.entity_id = Some(entity_id);
                                        editor_context.keyframe_dialog.property_name = name.clone();
                                        editor_context.keyframe_dialog.keyframe_index = idx;
                                        editor_context.keyframe_dialog.time = kf.time.into_inner();
                                        editor_context.keyframe_dialog.value =
                                            kf.value.get_as::<f64>().unwrap_or(0.0);
                                        editor_context.keyframe_dialog.easing = kf.easing.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Action::None => {}
    }

    if should_push_history {
        if let Ok(proj_read) = project.read() {
            history_manager.push_project_state(proj_read.clone());
        }
    }
}
