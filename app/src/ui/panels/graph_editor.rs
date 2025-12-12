use egui::{
    vec2, Color32, Pos2, Sense, Stroke, Ui,
};
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyValue};
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::state::context::EditorContext;

// Move Action enum outside to be accessible
enum Action {
    Select(String, usize),
    Move(String, usize, f64, f64), // prop_key, index, new_time, new_value
    Add(String, f64, f64), // prop_key, time, value
    None
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

    // SCOPE FOR READ LOCK
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

        // Filter properties to visualize (Only numerical values for now)
        let properties_to_plot: Vec<(&String, &Property)> = entity
            .properties
            .iter()
            .filter(|(_, p)| {
                 // For now, visualize anything with keyframes or constant numbers
                 match p.evaluator.as_str() {
                     "keyframe" => true,
                     "constant" => matches!(p.value(), Some(PropertyValue::Number(_))),
                     _ => false,
                 }
            })
            .collect();
        
        if properties_to_plot.is_empty() {
            ui.label("No animatable properties found.");
            return; // Returns from closure? No, from function.
            // Wait, if we return here, 'action' is None, loop below does nothing. Correct.
        }
        
        // Need to capture action in closure below, so we need RefCell or just assignment?
        // Closure needs to be FnMut to mutate action.
        
        // Grid Layout constants
        let pixels_per_frame: f32 = 10.0; // Zoom level X
        let pixels_per_unit: f32 = 1.0;  // Zoom level Y (auto-scaling TODO)
        let height = ui.available_height().max(200.0);
        
        // Draw Graph Area
        egui::ScrollArea::both().show(ui, |ui| {
            let (response, painter) = ui.allocate_painter(vec2(ui.available_width(), height), Sense::click_and_drag());
            let rect = response.rect;
            
            // Background
            painter.rect_filled(rect, 0.0, Color32::from_gray(30));

            // Draw Time Grid (Vertical Lines)
            let total_frames = (composition.duration * composition.fps) as i64;
            for f in 0..total_frames {
                let x = rect.min.x + (f as f32 * pixels_per_frame);
                if x > rect.max.x { break; }
                 if x < rect.min.x { continue; }
                
                let color = if f % 10 == 0 { Color32::from_gray(80) } else { Color32::from_gray(50) };
                painter.line_segment([
                    Pos2::new(x, rect.min.y),
                    Pos2::new(x, rect.max.y)
                ], Stroke::new(1.0, color));
            }

            // Visualize Properties
            let mut color_cycle = [
                Color32::RED,
                Color32::GREEN,
                Color32::BLUE,
                Color32::YELLOW,
                Color32::CYAN,
                Color32::MAGENTA,
            ].iter().cycle();

            let zero_y = rect.min.y + rect.height() / 2.0;

            for (name, property) in properties_to_plot {
                let color = *color_cycle.next().unwrap();
                
                match property.evaluator.as_str() {
                    "constant" => {
                        if let Some(val) = property.value().and_then(|v| v.get_as::<f64>()) {
                             let y = zero_y - (val as f32 * pixels_per_unit);
                             painter.line_segment([
                                 Pos2::new(rect.min.x, y),
                                 Pos2::new(rect.max.x, y)
                             ], Stroke::new(2.0, color));
                             
                             painter.text(Pos2::new(rect.min.x + 10.0, y - 10.0), egui::Align2::LEFT_BOTTOM, format!("{}: {:.2}", name, val), egui::FontId::default(), color);
                             
                             // Double click on constant line -> Add Keyframe?
                             if response.double_clicked() {
                                 if let Some(pointer_pos) = response.interact_pointer_pos() {
                                     // Check if click is near the line vertically
                                     if (pointer_pos.y - y).abs() < 5.0 {
                                          // Calculate time
                                          let relative_x = pointer_pos.x - rect.min.x;
                                          let frame = relative_x / pixels_per_frame;
                                          let time = frame as f64 / composition.fps;
                                          
                                          // Only if no action taken yet
                                          if let Action::None = action {
                                               action = Action::Add(name.clone(), time, val);
                                          }
                                     }
                                 }
                             }
                        }
                    },
                    "keyframe" => {
                         let keyframes = property.keyframes();
                         if keyframes.is_empty() { continue; }
                         
                         let mut sorted_kf = keyframes.clone();
                         sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                         
                         let mut path_points = Vec::new();

                         for (i, kf) in sorted_kf.iter().enumerate() {
                             let t = kf.time.into_inner();
                             let val = kf.value.get_as::<f64>().unwrap_or(0.0); // Fallback if not number
                             
                             let x = rect.min.x + ((t * composition.fps) as f32 * pixels_per_frame);
                             let y = zero_y - (val as f32 * pixels_per_unit);
                             let kf_pos = Pos2::new(x, y);
                             
                             path_points.push(kf_pos);
                             
                             // Draw keyframe dot
                             let is_selected = editor_context.interaction.selected_keyframe.as_ref()
                                 .map_or(false, |(s_name, s_idx)| s_name == name && *s_idx == i);
                                 
                             let dot_color = if is_selected { Color32::WHITE } else { color };
                             let radius = if is_selected { 6.0 } else { 4.0 };
                             
                             painter.circle_filled(kf_pos, radius, dot_color);
                             
                             // Interaction Check
                             // Hit test
                             if let Some(pointer_pos) = response.hover_pos() {
                                 if pointer_pos.distance(kf_pos) < 10.0 {
                                     // Hovering keyframe
                                     if response.clicked() {
                                         // Select
                                         action = Action::Select(name.clone(), i);
                                     }
                                 }
                             }
                             
                             // Dragging
                             if is_selected && response.dragged() {
                                  // Calculate delta
                                  let delta = response.drag_delta();
                                  let frame_delta = delta.x / pixels_per_frame;
                                  let time_delta = frame_delta as f64 / composition.fps;
                                  
                                  let val_delta = -(delta.y / pixels_per_unit) as f64; // Invert Y
                                  
                                  let new_time = (t + time_delta).max(0.0);
                                  let new_val = val + val_delta;
                                  
                                  action = Action::Move(name.clone(), i, new_time, new_val);
                             }
                         }
                         
                         if path_points.len() > 1 {
                             painter.add(egui::Shape::line(path_points, Stroke::new(2.0, color)));
                         }
                         
                         if let Some(first) = keyframes.first() {
                              let val = first.value.get_as::<f64>().unwrap_or(0.0);
                              let y = zero_y - (val as f32 * pixels_per_unit);
                               painter.text(Pos2::new(rect.min.x + 10.0, y - 10.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::default(), color);
                         }
                         
                        // Add keyframe logic (TODO: verify on curve)
                         if response.double_clicked() {
                             if let Some(_pointer_pos) = response.interact_pointer_pos() {
                                  // TODO: Add keyframe on curve double click
                             }
                         }
                    },
                    _ => {}
                }
            }
            
            // Background deselect logic
             // If we are here, we didn't click a keyframe (loop finished, if action is still None for Select)
             if let Action::None = action {
                 if response.clicked() && response.hover_pos().is_some() {
                      // Only if clicked on nothing specific? 
                      // Wait, if we selected a keyframe earlier in loop, action would be set.
                      // Since we process all properties, the last property loop might overwrite None?
                      // No, action is initialized to None outside. One set, we should probably stop? 
                      // Or prioritizing last one.
                      // But if action IS None here, it means no keyframe was clicked.
                      // So we can clear selection.
                      // BUT, we can't clear it via Action::None because that's the default state.
                      // Use a new Action::Deselect?
                 }
             }

            // Draw Playhead / Current Time
            let fps = composition.fps;
            let current_frame = editor_context.timeline.current_time as f64 * fps;
            let cursor_x = rect.min.x + (current_frame as f32 * pixels_per_frame);
            
            if cursor_x >= rect.min.x && cursor_x <= rect.max.x {
                painter.line_segment([
                    Pos2::new(cursor_x, rect.min.y),
                    Pos2::new(cursor_x, rect.max.y)
                ], Stroke::new(2.0, Color32::RED));
            }

        });
        
    } // READ LOCK DROPPED HERE
    
    // Handle Actions (Outside Read Lock)
    match action {
        Action::Select(name, idx) => {
            editor_context.interaction.selected_keyframe = Some((name, idx));
        },
         Action::Move(name, idx, new_time, new_val) => {
             // Update Keyframe
             use ordered_float::OrderedFloat;
             let _ = project_service.update_keyframe(
                 comp_id, track_id, entity_id, 
                 &name, idx, new_time, Some(PropertyValue::Number(OrderedFloat(new_val)))
             );
        },
        Action::Add(name, time, val) => {
            use ordered_float::OrderedFloat;
             let _ = project_service.add_keyframe(
                 comp_id, track_id, entity_id,
                 &name, time, PropertyValue::Number(OrderedFloat(val)),
                 library::animation::EasingFunction::Linear
             );
        },
        Action::None => {
             // Handle deselection if needed, but risky to infer from None unless we have explicit Deselect action
             // For now, let's skip auto-deselect on background click to avoid accidents.
        }
    }
}
