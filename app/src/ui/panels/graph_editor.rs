use egui::{
    vec2, Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use library::animation::EasingFunction;
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyValue};
use library::service::project_service::ProjectService;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

use crate::state::context::EditorContext;

enum Action {
    Select(String, usize),
    Move(String, usize, f64, f64), // prop_key, index, new_time, new_value
    Add(String, f64, f64),         // prop_key, time, value
    SetEasing(String, usize, EasingFunction),
    None,
}

pub fn graph_editor_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    ui.heading("Graph Editor");

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

        let properties_to_plot: Vec<(&String, &Property)> = entity
            .properties
            .iter()
            .filter(|(_, p)| match p.evaluator.as_str() {
                "keyframe" => true,
                "constant" => matches!(p.value(), Some(PropertyValue::Number(_))),
                _ => false,
            })
            .collect();

        if properties_to_plot.is_empty() {
            ui.label("No animatable properties found.");
            return;
        }

        // --- Interaction & View Control ---
        // Handle Zoom (Ctrl + Scroll)
        if ui.input(|i| i.modifiers.ctrl) {
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if zoom_delta != 1.0 {
                editor_context.graph_editor.zoom_x *= zoom_delta;
                editor_context.graph_editor.zoom_y *= zoom_delta;
            }
        }
        // Handle separate axis zoom if needed (e.g. Alt for Y, Shift for Time? Standard usually Ctrl+Scroll is both or Time)
        // For now coupled zoom is fine, or maybe just Zoom X with Scroll, Alt+Scroll for Y?
        // Let's stick to basic Zoom X/Y together for now, or just X if that's more useful for timeline.
        // Actually, for value graphs, independent zoom is crucial.
        // Let's use:
        // - Ctrl + Scroll: Zoom Time (X)
        // - Alt + Scroll: Zoom Value (Y) (if we want explicit control)
        // But user just said "Pan/Zoom".
        
        // Let's refine:
        // Ctrl+Scroll -> Zoom Both? Or X?
        // Let's enable Mouse Wheel for Pan (standard) and Ctrl+Wheel for Zoom.

        let pixels_per_second = editor_context.graph_editor.zoom_x;
        let pixels_per_unit = editor_context.graph_editor.zoom_y;
        
        let available_size = ui.available_size();
        let (response, painter) = ui.allocate_painter(available_size, Sense::click_and_drag());
        let rect = response.rect;

        // Pan Logic (Middle click or standard Drag on background if not selecting)
        if response.dragged_by(PointerButton::Middle) || (response.dragged() && editor_context.interaction.selected_keyframe.is_none()) {
             editor_context.graph_editor.pan += response.drag_delta();
        }

        // Clip Rect to area
        let clip_rect = rect;
        painter.rect_filled(clip_rect, 0.0, Color32::from_gray(30));
        
        let to_screen_pos = |time: f64, value: f64| -> Pos2 {
             let x = rect.min.x + editor_context.graph_editor.pan.x + (time as f32 * pixels_per_second);
             // Invert Y: value goes UP, screen Y goes DOWN
             // Center Y around middle of rect initially? Or just pan offset.
             // Let's assume pan.y = 0 means value 0 is at bottom? Or center?
             // Let's put value 0 at rect.center().y + pan.y
             let zero_y = rect.center().y + editor_context.graph_editor.pan.y;
             let y = zero_y - (value as f32 * pixels_per_unit);
             Pos2::new(x, y)
        };
        
        let from_screen_pos = |pos: Pos2| -> (f64, f64) {
             let x = pos.x;
             let time = (x - rect.min.x - editor_context.graph_editor.pan.x) / pixels_per_second;
             
             let zero_y = rect.center().y + editor_context.graph_editor.pan.y;
             let y = pos.y;
             let value = (zero_y - y) / pixels_per_unit;
             (time as f64, value as f64)
        };

        // Draw Time Grid
        let start_time = -editor_context.graph_editor.pan.x / pixels_per_second;
        let end_time = (rect.width() - editor_context.graph_editor.pan.x) / pixels_per_second;
        // Step size based on zoom
        let step_time = if pixels_per_second > 200.0 { 0.1 } else if pixels_per_second > 50.0 { 1.0 } else { 5.0 };
        
        let start_step = (start_time / step_time).floor() as i64;
        let end_step = (end_time / step_time).ceil() as i64;
        
        for i in start_step..=end_step {
            let t = i as f64 * step_time as f64;
            let x = to_screen_pos(t, 0.0).x;
            if x >= rect.min.x && x <= rect.max.x {
                 let color = Color32::from_gray(if i % 10 == 0 { 60 } else { 40 });
                 painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], Stroke::new(1.0, color));
                 if i % 10 == 0 || pixels_per_second > 200.0 {
                     painter.text(Pos2::new(x + 2.0, rect.max.y - 15.0), egui::Align2::LEFT_BOTTOM, format!("{:.1}s", t), egui::FontId::default(), Color32::GRAY);
                 }
            }
        }
        
        // Horizontal (Value) Grid - Optional, but helpful. Draw 0 line.
        let zero_y = to_screen_pos(0.0, 0.0).y;
        if zero_y >= rect.min.y && zero_y <= rect.max.y {
            painter.line_segment([Pos2::new(rect.min.x, zero_y), Pos2::new(rect.max.x, zero_y)], Stroke::new(1.0, Color32::from_gray(80)));
        }

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
                             [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
                             Stroke::new(2.0, color),
                         );
                         painter.text(Pos2::new(rect.min.x + 10.0, y - 5.0), egui::Align2::LEFT_BOTTOM, format!("{}: {:.2}", name, val), egui::FontId::default(), color);
                         
                         // Double Click to add keyframe (Constant -> Keyframe)
                         if response.double_clicked() {
                             if let Some(pointer_pos) = response.interact_pointer_pos() {
                                  if (pointer_pos.y - y).abs() < 5.0 {
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
                    for i in 0..sorted_kf.len() {
                         let kf = &sorted_kf[i];
                         let t = kf.time.into_inner();
                         let val = kf.value.get_as::<f64>().unwrap_or(0.0);
                         path_points.push(to_screen_pos(t, val));
                         
                         // TODO: Curve interpolation visualization (subdivide lines)
                         // For now straight lines between keyframes
                    }
                    if path_points.len() > 1 {
                        painter.add(egui::Shape::line(path_points, Stroke::new(2.0, color)));
                    }
                    
                    // Draw Keyframe Points & Handle Interaction
                    for (i, kf) in sorted_kf.iter().enumerate() {
                         let t = kf.time.into_inner();
                         let val = kf.value.get_as::<f64>().unwrap_or(0.0);
                         let kf_pos = to_screen_pos(t, val);
                         
                         // Interaction area
                         let point_rect = Rect::from_center_size(kf_pos, Vec2::splat(12.0));
                         let point_id = response.id.with(name).with(i);
                         let point_response = ui.interact(point_rect, point_id, Sense::click_and_drag());
                         
                         let is_selected = editor_context.interaction.selected_keyframe.as_ref()
                             .map_or(false, |(s_name, s_idx)| s_name == name && *s_idx == i);

                         // Draw Dot
                         let dot_color = if is_selected { Color32::WHITE } else { color };
                         let radius = if is_selected { 6.0 } else { 4.0 };
                         painter.circle_filled(kf_pos, radius, dot_color);
                         
                         // Selection
                         if point_response.clicked() {
                             action = Action::Select(name.clone(), i);
                         }
                         
                         // Context Menu (Easing)
                         point_response.context_menu(|ui| {
                             ui.label(format!("Keyframe {} - {}", i, name));
                             ui.separator();
                             ui.label("Easing Function:");
                             if ui.button("Linear").clicked() {
                                 action = Action::SetEasing(name.clone(), i, EasingFunction::Linear);
                             }
                             if ui.button("EaseIn").clicked() {
                                 action = Action::SetEasing(name.clone(), i, EasingFunction::EaseInCubic); // Map "EaseIn" to cubic for better feel or Quad
                             }
                             if ui.button("EaseOut").clicked() {
                                 action = Action::SetEasing(name.clone(), i, EasingFunction::EaseOutCubic);
                             }
                             if ui.button("EaseInOut").clicked() {
                                 action = Action::SetEasing(name.clone(), i, EasingFunction::EaseInOutCubic);
                             }
                         });
                    
                         // Dragging
                         if is_selected && point_response.dragged() {
                              let (new_t, new_val) = from_screen_pos(kf_pos + point_response.drag_delta());
                              action = Action::Move(name.clone(), i, new_t.max(0.0), new_val);
                         }
                    }
                    
                    // Add Keyframe (Double Click on Curve/Background?)
                    // It's hard to hit the exact curve line. Double click anywhere to add keyframe to CLOSEST curve?
                    // Or double click near the curve.
                    // For now, let's say "Double click sets keyframe for THIS property if near curve"
                    if response.double_clicked() {
                         if let Some(pointer_pos) = response.interact_pointer_pos() {
                              // Check distance to curve segments
                                let (t, v) = from_screen_pos(pointer_pos);
                                let mut best_dist = f32::MAX;
                                
                                // Simple distance check to interpolated segments
                                for w in 0..sorted_kf.len().saturating_sub(1) {
                                     let p1 = to_screen_pos(sorted_kf[w].time.into_inner(), sorted_kf[w].value.get_as::<f64>().unwrap_or(0.0));
                                     let p2 = to_screen_pos(sorted_kf[w+1].time.into_inner(), sorted_kf[w+1].value.get_as::<f64>().unwrap_or(0.0));
                                     
                                     // Point to line segment distance
                                     // ... implementing simple geometry check
                                     let l2 = p1.distance_sq(p2);
                                     let dist = if l2 == 0.0 {
                                          p1.distance(pointer_pos)
                                     } else {
                                          let t_proj = ((pointer_pos - p1).dot(p2 - p1) / l2).clamp(0.0, 1.0);
                                          let proj = p1 + (p2 - p1) * t_proj;
                                          pointer_pos.distance(proj)
                                     };
                                     if dist < best_dist { best_dist = dist; }
                                }
                                
                                // Also check endpoints (flat line extension?)
                                // If close enough
                                if best_dist < 10.0 {
                                      if let Action::None = action {
                                           action = Action::Add(name.clone(), t.max(0.0), v);
                                      }
                                }
                         }
                    }
                }
                _ => {}
            }
        }
        
        // Draw Playhead
        let t_cursor = editor_context.timeline.current_time as f64;
        let x_cursor = to_screen_pos(t_cursor, 0.0).x;
        if x_cursor >= rect.min.x && x_cursor <= rect.max.x {
            painter.line_segment(
                [Pos2::new(x_cursor, rect.min.y), Pos2::new(x_cursor, rect.max.y)],
                Stroke::new(2.0, Color32::RED)
            );
        }
    }

    // Process Actions
    match action {
        Action::Select(name, idx) => {
            editor_context.interaction.selected_keyframe = Some((name, idx));
        }
        Action::Move(name, idx, new_time, new_val) => {
            let _ = project_service.update_keyframe(
                comp_id, track_id, entity_id,
                &name, idx, Some(new_time), 
                Some(PropertyValue::Number(OrderedFloat(new_val))),
                None
            );
        }
        Action::Add(name, time, val) => {
            let _ = project_service.add_keyframe(
                comp_id, track_id, entity_id,
                &name, time, PropertyValue::Number(OrderedFloat(val)),
                EasingFunction::Linear
            );
        }
        Action::SetEasing(name, idx, easing) => {
             let _ = project_service.update_keyframe(
                 comp_id, track_id, entity_id,
                 &name, idx, 
                 None, // Keep existing time
                 None, // Keep existing value
                 Some(easing)
             );
        }
        Action::None => {}
    }
}
