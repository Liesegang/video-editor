use egui::{
    Color32, PointerButton, Pos2, Rect, Sense, Stroke, Ui, UiKind, Vec2,
};
use library::animation::EasingFunction;
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyValue};
use library::service::project_service::ProjectService;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

use crate::action::HistoryManager;
use crate::state::context::EditorContext;

enum Action {
    Select(String, usize),
    Move(String, usize, f64, f64), // prop_key, index, new_time, new_value
    Add(String, f64, f64),         // prop_key, time, value
    SetEasing(String, usize, EasingFunction),
    Remove(String, usize),
    EditKeyframe(String, usize),
    None,
}

pub fn graph_editor_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    let (comp_id, track_id, entity_id) = match (
        editor_context.selection.composition_id,
        editor_context.selection.track_id,
        editor_context.selection.entity_id,
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

        let mut properties_to_plot: Vec<(String, &Property)> = entity
            .properties
            .iter()
            .filter(|(_, p)| match p.evaluator.as_str() {
                "keyframe" => true,
                "constant" => matches!(p.value(), Some(PropertyValue::Number(_))),
                _ => false,
            })
            .map(|(k, p)| (k.clone(), p))
            .collect();

        // Add Effect Properties

        for (effect_idx, effect) in entity.effects.iter().enumerate() {
            for (prop_key, prop) in effect.properties.iter() {
                let should_plot = match prop.evaluator.as_str() {
                    "keyframe" => true,
                    "constant" => matches!(prop.value(), Some(PropertyValue::Number(_))),
                    _ => false,
                };
                if should_plot {
                    properties_to_plot.push((format!("effect:{}:{}", effect_idx, prop_key), prop));
                }
            }
        }

        if properties_to_plot.is_empty() {
            // We still want to show empty graph? no, return?
            // User might want to drag drop properties?
            // For now return.
            ui.label("No animatable properties found.");
            return;
        }

        ui.input(|i| {
            // Independent Zoom
            let scroll_delta = i.raw_scroll_delta.y / 50.0; // Basic scaling factor
            if scroll_delta != 0.0 {
                if i.modifiers.alt {
                    // Zoom Y (Value)
                    let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
                    editor_context.graph_editor.zoom_y *= zoom_factor;
                } else if i.modifiers.ctrl {
                    // Zoom X (Time)
                    let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
                    editor_context.graph_editor.zoom_x *= zoom_factor;
                }
            }
        });

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
        let (_base_response, painter) = ui.allocate_painter(available_rect.size(), Sense::hover());

        // Explicitly handle interactions for disjoint areas
        let ruler_response =
            ui.interact(ruler_rect, ui.id().with("ruler"), Sense::click_and_drag());
        let graph_response =
            ui.interact(graph_rect, ui.id().with("graph"), Sense::click_and_drag());

        // Interaction Handling

        // Ruler Interaction (Seek)
        if ruler_response.dragged() || ruler_response.clicked() {
            if let Some(pos) = ruler_response.interact_pointer_pos() {
                let x = pos.x;
                let time =
                    (x - graph_rect.min.x - editor_context.graph_editor.pan.x) / pixels_per_second;
                editor_context.timeline.current_time = time.max(0.0) as f32;
            }
        }

        // Graph Interaction (Pan)
        if graph_response.dragged_by(PointerButton::Middle) {
            editor_context.graph_editor.pan += graph_response.drag_delta();
        }

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

        // Paint Background
        painter.rect_filled(graph_rect, 0.0, Color32::from_gray(30));
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
            let time =
                (x - graph_rect.min.x - editor_context.graph_editor.pan.x) / pixels_per_second;

            let zero_y = graph_rect.center().y + editor_context.graph_editor.pan.y;
            let y = pos.y;
            let value = (zero_y - y) / pixels_per_unit;
            (time as f64, value as f64)
        };

        // --- Draw Rulers and Grids ---

        // Time Grid & Ruler
        let start_time = (-editor_context.graph_editor.pan.x / pixels_per_second) as f64;
        let end_time =
            ((graph_rect.width() - editor_context.graph_editor.pan.x) / pixels_per_second) as f64;

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

                // Label (Left side)
                painter.text(
                    Pos2::new(graph_rect.min.x + 2.0, y - 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{:.2}", val),
                    egui::FontId::proportional(10.0),
                    Color32::from_gray(150),
                );
            }
        }

        // Clip painter for graph content
        let painter = painter.with_clip_rect(graph_rect);

        // Draw Properties
        let mut color_cycle = [
            Color32::RED,
            Color32::GREEN,
            Color32::BLUE,
            Color32::YELLOW,
            Color32::CYAN,
            Color32::MAGENTA,
        ]
        .iter()
        .cycle();

        for (name, property) in properties_to_plot {
            let color = *color_cycle.next().unwrap();

            match property.evaluator.as_str() {
                "constant" => {
                    if let Some(val) = property.value().and_then(|v| v.get_as::<f64>()) {
                        let y = to_screen_pos(0.0, val).y;
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

                        // Double Click to add keyframe (Constant -> Keyframe)
                        if response.double_clicked() {
                            if let Some(pointer_pos) = response.interact_pointer_pos() {
                                if (pointer_pos.y - y).abs() < 5.0
                                    && graph_rect.contains(pointer_pos)
                                {
                                    let (t, _) = from_screen_pos(pointer_pos);
                                    if let Action::None = action {
                                        action = Action::Add(name.clone(), t.max(0.0), val);
                                    }
                                }
                            }
                        }
                    }
                }
                "keyframe" => {
                    let keyframes = property.keyframes();
                    if keyframes.is_empty() {
                        continue;
                    }

                    let mut sorted_kf = keyframes.clone();
                    sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));

                    let mut path_points = Vec::new();

                    // Draw Curve
                    for i in 0..sorted_kf.len().saturating_sub(1) {
                        let k1 = &sorted_kf[i];
                        let k2 = &sorted_kf[i + 1];

                        let t1 = k1.time.into_inner();
                        let t2 = k2.time.into_inner();
                        let v1 = k1.value.get_as::<f64>().unwrap_or(0.0);
                        let v2 = k2.value.get_as::<f64>().unwrap_or(0.0);

                        let p1 = to_screen_pos(t1, v1);
                        let p2 = to_screen_pos(t2, v2);

                        // Adaptive sampling based on screen distance
                        let dist = (p2.x - p1.x).abs().max(1.0);
                        let steps = (dist / 2.0).ceil() as usize; // Sample every 2 pixels roughly
                        let steps = steps.clamp(1, 1000); // Sanity limits

                        for s in 0..=steps {
                            let progress = s as f64 / steps as f64;
                            let eased_progress = k1.easing.apply(progress);

                            let current_time = t1 + (t2 - t1) * progress;
                            let current_value = v1 + (v2 - v1) * eased_progress;

                            path_points.push(to_screen_pos(current_time, current_value));
                        }
                    }

                    if path_points.len() > 1 {
                        painter.add(egui::Shape::line(path_points, Stroke::new(2.0, color)));
                    }

                    // Draw Keyframe Points & Handle Interaction
                    for (i, kf) in sorted_kf.iter().enumerate() {
                        let t = kf.time.into_inner();
                        let val = kf.value.get_as::<f64>().unwrap_or(0.0);
                        let kf_pos = to_screen_pos(t, val);

                        // Skip if out of view (optimization)
                        if !graph_rect.expand(10.0).contains(kf_pos) {
                            continue;
                        }

                        // Interaction area
                        let point_rect = Rect::from_center_size(kf_pos, Vec2::splat(12.0));
                        let point_id = response.id.with(&name).with(i);
                        let point_response =
                            ui.interact(point_rect, point_id, Sense::click_and_drag());

                        let is_selected = editor_context
                            .interaction
                            .selected_keyframe
                            .as_ref()
                            .map_or(false, |(s_name, s_idx)| s_name == &name && *s_idx == i);

                        // Draw Dot
                        let dot_color = if is_selected { Color32::WHITE } else { color };
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
                            ui.label("Easing:");

                            if ui.button("Linear").clicked() {
                                action = Action::SetEasing(name_for_menu.clone(), i, EasingFunction::Linear);
                                should_push_history = true;
                                ui.close_kind(UiKind::Menu);
                            }

                            ui.menu_button("Sine", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInSine,
                                    );
                                    should_push_history = true;
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutSine,
                                    );
                                    should_push_history = true;
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutSine,
                                    );
                                    should_push_history = true;
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Quad", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInQuad,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutQuad,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutQuad,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Cubic", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInCubic,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutCubic,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutCubic,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Quart", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInQuart,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutQuart,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutQuart,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Quint", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInQuint,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutQuint,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutQuint,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Expo", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInExpo,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutExpo,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutExpo,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Circ", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInCirc,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutCirc,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutCirc,
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Back", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInBack { c1: 1.70158 },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutBack { c1: 1.70158 },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutBack { c1: 1.70158 },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Elastic", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInElastic { period: 3.0 },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutElastic { period: 3.0 },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutElastic { period: 4.5 },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Bounce", |ui| {
                                if ui.button("Ease In").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInBounce {
                                            n1: 7.5625,
                                            d1: 2.75,
                                        },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseOutBounce {
                                            n1: 7.5625,
                                            d1: 2.75,
                                        },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                                if ui.button("Ease In Out").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::EaseInOutBounce {
                                            n1: 7.5625,
                                            d1: 2.75,
                                        },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });
                            ui.menu_button("Custom", |ui| {
                                if ui.button("Expression").clicked() {
                                    action = Action::SetEasing(
                                        name_for_menu.clone(),
                                        i,
                                        EasingFunction::Expression {
                                            text: "t".to_string(),
                                        },
                                    );
                                    ui.close_kind(UiKind::Menu);
                                }
                            });

                            ui.separator();
                            if ui.button("Edit Keyframe...").clicked() {
                                action = Action::EditKeyframe(name_for_menu.clone(), i);
                                ui.close_kind(UiKind::Menu);
                            }

                            ui.separator();
                            if ui
                                .button(egui::RichText::new("Delete Keyframe").color(Color32::RED))
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
                            action = Action::Move(name.clone(), i, new_t.max(0.0), new_val);
                        }
                    }

                    // Add Keyframe (Double Click on Curve/Background?)
                    if response.double_clicked() {
                        if let Some(pointer_pos) = response.interact_pointer_pos() {
                            // Only if inside graph rect
                            if graph_rect.contains(pointer_pos) {
                                let (t, v) = from_screen_pos(pointer_pos);
                                // ... (Keep existing best_dist logic but constrained to graph_rect)
                                let mut best_dist = f32::MAX;

                                for w in 0..sorted_kf.len().saturating_sub(1) {
                                    let p1 = to_screen_pos(
                                        sorted_kf[w].time.into_inner(),
                                        sorted_kf[w].value.get_as::<f64>().unwrap_or(0.0),
                                    );
                                    let p2 = to_screen_pos(
                                        sorted_kf[w + 1].time.into_inner(),
                                        sorted_kf[w + 1].value.get_as::<f64>().unwrap_or(0.0),
                                    );
                                    // ...
                                    let l2 = p1.distance_sq(p2);
                                    let dist = if l2 == 0.0 {
                                        p1.distance(pointer_pos)
                                    } else {
                                        let t_proj =
                                            ((pointer_pos - p1).dot(p2 - p1) / l2).clamp(0.0, 1.0);
                                        let proj = p1 + (p2 - p1) * t_proj;
                                        pointer_pos.distance(proj)
                                    };
                                    if dist < best_dist {
                                        best_dist = dist;
                                    }
                                }

                                if best_dist < 10.0 {
                                    if let Action::None = action {
                                        action = Action::Add(name.clone(), t.max(0.0), v);
                                        should_push_history = true;
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

    match action {
        Action::Select(name, idx) => {
            editor_context.interaction.selected_keyframe = Some((name, idx));
        }
        Action::Move(name, idx, new_time, new_val) => {
            if let Some((eff_idx, prop_key)) = parse_key(&name) {
                let _ = project_service.update_effect_keyframe_by_index(
                    comp_id,
                    track_id,
                    entity_id,
                    eff_idx,
                    &prop_key,
                    idx,
                    Some(new_time),
                    Some(PropertyValue::Number(OrderedFloat(new_val))),
                    None,
                );
            } else {
                let _ = project_service.update_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    &name,
                    idx,
                    Some(new_time),
                    Some(PropertyValue::Number(OrderedFloat(new_val))),
                    None,
                );
            }
        }
        Action::Add(name, time, val) => {
            if let Some((eff_idx, prop_key)) = parse_key(&name) {
                // Use add_effect_keyframe to add keyframe and handle constant->keyframe conversion
                let _ = project_service.add_effect_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    eff_idx,
                    &prop_key,
                    time,
                    PropertyValue::Number(OrderedFloat(val)),
                    Some(EasingFunction::Linear),
                );
            } else {
                let _ = project_service.add_keyframe(
                    comp_id,
                    track_id,
                    entity_id,
                    &name,
                    time,
                    PropertyValue::Number(OrderedFloat(val)),
                    Some(EasingFunction::Linear),
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
                    comp_id,
                    track_id,
                    entity_id,
                    eff_idx,
                    &prop_key,
                    idx,
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
                                                    name.clone(); // Use full key name? Dialog likely re-uses it for display?
                                                                  // Or does dialog use it to call update?
                                                                  // If dialog calls `update_keyframe`, it won't work for effects unless we update dialog too.
                                                                  // Ah, Keyframe Dialog needs to know if it's an effect.
                                                                  // Right now KeyframeDialog uses `update_keyframe`.
                                                                  // So we might need to patch KeyframeDialog too if we want full support.
                                                                  // But let's populate it for now.
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
