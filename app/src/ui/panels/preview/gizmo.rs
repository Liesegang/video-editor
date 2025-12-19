use crate::model::ui_types::GizmoHandle;
use crate::state::context::EditorContext;
use crate::ui::panels::preview::clip::PreviewClip;
use egui::{CursorIcon, Pos2, Rect, Sense, Ui, Vec2};
use library::model::project::project::Project;
use library::model::project::property::{PropertyValue, Vec2 as PropVec2};
use library::EditorService as ProjectService;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

pub fn handle_gizmo_interaction(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &ProjectService,
    history_manager: &mut crate::action::HistoryManager,
    pointer_pos: Option<Pos2>,
    to_world: impl Fn(Pos2) -> Pos2,
) -> bool {
    let mut interacted_with_gizmo = false;

    // Extract Gizmo Information first to avoid double borrow of editor_context
    let gizmo_drag_data = if let Some(state) = &editor_context.interaction.gizmo_state {
        Some((
            state.start_mouse_pos,
            state.active_handle,
            state.original_position,
            state.original_scale_x,
            state.original_scale_y,
            state.original_rotation,
            state.original_width,
            state.original_height,
            state.original_anchor_x,
            state.original_anchor_y,
        ))
    } else {
        None
    };

    if let Some((
        start_mouse_pos,
        active_handle,
        orig_pos,
        orig_sx,
        orig_sy,
        orig_rot,
        orig_w,
        orig_h,
        _orig_ax,
        _orig_ay,
    )) = gizmo_drag_data
    {
        if ui.input(|i| i.pointer.any_released()) {
            editor_context.interaction.gizmo_state = None;
            interacted_with_gizmo = true; // Prevent click-through to selection logic on release

            // Push project state to history
            if let Ok(proj) = project.read() {
                history_manager.push_project_state(proj.clone());
            }
        } else if let Some(mouse_pos) = pointer_pos {
            interacted_with_gizmo = true;

            // Re-acquire selected entity data
            if let Some(selected_id) = editor_context.selection.last_selected_entity_id {
                // Clone needed properties to avoid borrow issues
                let (comp_id, track_id, current_props) = if let Ok(proj_read) = project.read() {
                    if let Some(comp) = editor_context.get_current_composition(&proj_read) {
                        if let Some(track) = comp
                            .tracks
                            .iter()
                            .find(|t| t.clips.iter().any(|c| c.id == selected_id))
                        {
                            if let Some(clip) = track.clips.iter().find(|c| c.id == selected_id) {
                                (Some(comp.id), Some(track.id), Some(clip.properties.clone()))
                            } else {
                                (None, None, None)
                            }
                        } else {
                            (None, None, None)
                        }
                    } else {
                        (None, None, None)
                    }
                } else {
                    (None, None, None)
                };

                if let (Some(comp_id), Some(track_id), Some(_)) = (comp_id, track_id, current_props)
                {
                    // Calculate Delta (World Space)
                    let start_world = to_world(start_mouse_pos);
                    let current_world = to_world(mouse_pos);
                    let delta_world = current_world - start_world;

                    let modifiers = ui.input(|i| i.modifiers);
                    let keep_aspect_ratio = modifiers.shift;
                    let center_scale = modifiers.alt;

                    // Logic depends on handle
                    let mut new_scale_x = orig_sx;
                    let mut new_scale_y = orig_sy;
                    let mut new_pos_x = orig_pos[0];
                    let mut new_pos_y = orig_pos[1];
                    let mut new_rotation = orig_rot;

                    let base_w = orig_w;
                    let base_h = orig_h;

                    // Helper: Rotate vector by angle
                    let rotate_vec = |v: Vec2, angle_deg: f32| -> Vec2 {
                        let rad = angle_deg.to_radians();
                        let c = rad.cos();
                        let s = rad.sin();
                        egui::vec2(v.x * c - v.y * s, v.x * s + v.y * c)
                    };

                    match active_handle {
                        GizmoHandle::Rotation => {
                            // Rotation Logic
                            // Center of rotation
                            let center = egui::pos2(orig_pos[0], orig_pos[1]);
                            let start_vec = start_world - center;
                            let current_vec = current_world - center;

                            let angle_start = start_vec.y.atan2(start_vec.x).to_degrees();
                            let angle_current = current_vec.y.atan2(current_vec.x).to_degrees();

                            new_rotation = orig_rot + (angle_current - angle_start);
                        }
                        _ => {
                            // Resize Logic
                            let rad = orig_rot.to_radians();
                            let c = rad.cos();
                            let s = rad.sin();

                            // Delta in aligned space
                            let dx = delta_world.x * c + delta_world.y * s;
                            let dy = -delta_world.x * s + delta_world.y * c;

                            let current_w = base_w * orig_sx / 100.0;
                            let current_h = base_h * orig_sy / 100.0;

                            let (sign_x, sign_y) = match active_handle {
                                GizmoHandle::TopLeft => (-1.0, -1.0),
                                GizmoHandle::Top => (0.0, -1.0),
                                GizmoHandle::TopRight => (1.0, -1.0),
                                GizmoHandle::Left => (-1.0, 0.0),
                                GizmoHandle::Right => (1.0, 0.0),
                                GizmoHandle::BottomLeft => (-1.0, 1.0),
                                GizmoHandle::Bottom => (0.0, 1.0),
                                GizmoHandle::BottomRight => (1.0, 1.0),
                                _ => (0.0, 0.0),
                            };

                            let scale_factor = if center_scale { 2.0 } else { 1.0 };
                            let raw_d_w = if sign_x != 0.0 {
                                dx * sign_x * scale_factor
                            } else {
                                0.0
                            };
                            let raw_d_h = if sign_y != 0.0 {
                                dy * sign_y * scale_factor
                            } else {
                                0.0
                            };

                            let mut next_w = current_w + raw_d_w;
                            let mut next_h = current_h + raw_d_h;

                            if keep_aspect_ratio {
                                let ratio = if current_h != 0.0 {
                                    current_w / current_h
                                } else {
                                    1.0
                                };

                                if sign_x != 0.0 && sign_y != 0.0 {
                                    // Corner
                                    if raw_d_w.abs() > raw_d_h.abs() {
                                        next_h = next_w / ratio;
                                    } else {
                                        next_w = next_h * ratio;
                                    }
                                } else if sign_x != 0.0 {
                                    // Left/Right: Width is dominant
                                    next_h = next_w / ratio;
                                } else if sign_y != 0.0 {
                                    // Top/Bottom: Height is dominant
                                    next_w = next_h * ratio;
                                }
                            }

                            let final_d_w = next_w - current_w;
                            let final_d_h = next_h - current_h;

                            if base_w > 0.0 {
                                new_scale_x = next_w / base_w * 100.0;
                            }
                            if base_h > 0.0 {
                                new_scale_y = next_h / base_h * 100.0;
                            }

                            if !center_scale {
                                let shift_x = sign_x * final_d_w / 2.0;
                                let shift_y = sign_y * final_d_h / 2.0;

                                let shift = rotate_vec(egui::vec2(shift_x, shift_y), orig_rot);
                                new_pos_x += shift.x;
                                new_pos_y += shift.y;
                            }
                        }
                    }

                    // Apply Updates
                    // Apply Updates
                    let current_time = editor_context.timeline.current_time as f64;

                    crate::utils::property::update_property(
                        project_service,
                        comp_id,
                        track_id,
                        selected_id,
                        "scale",
                        current_time,
                        PropertyValue::Vec2(PropVec2 {
                            x: OrderedFloat(new_scale_x as f64),
                            y: OrderedFloat(new_scale_y as f64),
                        }),
                    );
                    crate::utils::property::update_property(
                        project_service,
                        comp_id,
                        track_id,
                        selected_id,
                        "position",
                        current_time,
                        PropertyValue::Vec2(PropVec2 {
                            x: OrderedFloat(new_pos_x as f64),
                            y: OrderedFloat(new_pos_y as f64),
                        }),
                    );
                    crate::utils::property::update_number_property(
                        project_service,
                        comp_id,
                        track_id,
                        selected_id,
                        "rotation",
                        current_time,
                        new_rotation as f64,
                    );
                }
            }
        }
    }
    interacted_with_gizmo
}

pub fn draw_gizmo(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    gui_clips: &[PreviewClip],
    to_screen: impl Fn(Pos2) -> Pos2,
) {
    // Draw outlines for ALL selected entities to show multi-selection
    for selected_id in &editor_context.selection.selected_entities {
        // Skip the primary one if it will be drawn by the main gizmo logic.
        let is_primary = Some(*selected_id) == editor_context.selection.last_selected_entity_id;
        if is_primary {
            continue;
        }

        if let Some(gc) = gui_clips.iter().find(|gc| gc.id() == *selected_id) {
            if gc.clip.kind == library::model::project::TrackClipKind::Audio {
                continue;
            }

            let gizmo_color = egui::Color32::from_rgb(0, 200, 255).linear_multiply(0.5); // Dimmer for secondary
            draw_clip_box(ui, gc, |p| to_screen(p), gizmo_color, 1.0);
        }
    }

    if let Some(selected_id) = editor_context.selection.last_selected_entity_id {
        if let Some(gc) = gui_clips.iter().find(|gc| gc.id() == selected_id) {
            if gc.clip.kind == library::model::project::TrackClipKind::Audio {
                return;
            }

            // Draw Box (Primary)
            let gizmo_color = egui::Color32::from_rgb(0, 200, 255);
            let (corners, _center, rotation_rad, s_t) =
                draw_clip_box(ui, gc, |p| to_screen(p), gizmo_color, 2.0);

            // Draw Rotation Stick
            let painter = ui.painter();
            let rot_handle_dist = 10.0 / editor_context.view.zoom;
            let sin = rotation_rad.sin();
            let cos = rotation_rad.cos();
            let s_rot = s_t + egui::vec2(sin * rot_handle_dist, -cos * rot_handle_dist);

            painter.line_segment([s_t, s_rot], egui::Stroke::new(2.0, gizmo_color));
            painter.circle_filled(s_rot, 5.0, gizmo_color);

            // Draw Handles
            let handle_radius = 5.0;

            // Midpoints
            let s_b = corners[2].lerp(corners[3], 0.5);
            let s_l = corners[0].lerp(corners[3], 0.5);
            let s_r = corners[1].lerp(corners[2], 0.5);

            let handles = [
                (corners[0], GizmoHandle::TopLeft, CursorIcon::ResizeNwSe), // TL: 0
                (corners[1], GizmoHandle::TopRight, CursorIcon::ResizeNeSw), // TR: 1
                (corners[3], GizmoHandle::BottomLeft, CursorIcon::ResizeNeSw), // BL: 3
                (corners[2], GizmoHandle::BottomRight, CursorIcon::ResizeNwSe), // BR: 2
                (s_t, GizmoHandle::Top, CursorIcon::ResizeVertical),
                (s_b, GizmoHandle::Bottom, CursorIcon::ResizeVertical),
                (s_l, GizmoHandle::Left, CursorIcon::ResizeHorizontal),
                (s_r, GizmoHandle::Right, CursorIcon::ResizeHorizontal),
                (s_rot, GizmoHandle::Rotation, CursorIcon::Grab),
            ];

            for (pos, handle, cursor) in handles {
                painter.circle_filled(pos, handle_radius, gizmo_color);

                // Interaction Area
                let interact_rect = Rect::from_center_size(pos, Vec2::splat(handle_radius * 3.0)); // Larger hit area
                let response = ui.interact(interact_rect, ui.id().with(handle), Sense::drag());

                if response.hovered() {
                    ui.ctx().set_cursor_icon(cursor);
                }

                if response.drag_started() {
                    let base_w = gc.content_bounds.map(|b| b.2).unwrap_or(1920.0);
                    let base_h = gc.content_bounds.map(|b| b.3).unwrap_or(1080.0);

                    editor_context.interaction.gizmo_state =
                        Some(crate::state::context::GizmoState {
                            start_mouse_pos: response.hover_pos().unwrap_or(pos),
                            active_handle: handle,
                            original_position: [gc.transform.position.x as f32, gc.transform.position.y as f32],
                            original_scale_x: gc.transform.scale.x as f32,
                            original_scale_y: gc.transform.scale.y as f32,
                            original_rotation: gc.transform.rotation as f32,
                            original_anchor_x: gc.transform.anchor.x as f32,
                            original_anchor_y: gc.transform.anchor.y as f32,
                            original_width: base_w,
                            original_height: base_h,
                        });
                }
            }
        }
    }
}

fn draw_clip_box(
    ui: &Ui,
    gc: &PreviewClip,
    to_screen: impl Fn(Pos2) -> Pos2,
    color: egui::Color32,
    thickness: f32,
) -> ([Pos2; 4], Pos2, f32, Pos2) {
    let base_w = gc.content_bounds.map(|b| b.2).unwrap_or(1920.0);
    let base_h = gc.content_bounds.map(|b| b.3).unwrap_or(1080.0);
    let sx = gc.transform.scale.x as f32 / 100.0;
    let sy = gc.transform.scale.y as f32 / 100.0;

    let center = egui::pos2(gc.transform.position.x as f32, gc.transform.position.y as f32);
    let angle_rad = (gc.transform.rotation as f32).to_radians();
    let cos = angle_rad.cos();
    let sin = angle_rad.sin();

    let transform_point = |local_x: f32, local_y: f32| -> egui::Pos2 {
        let ox = local_x - gc.transform.anchor.x as f32;
        let oy = local_y - gc.transform.anchor.y as f32;
        let sx_ox = ox * sx;
        let sy_oy = oy * sy;
        let rx = sx_ox * cos - sy_oy * sin;
        let ry = sx_ox * sin + sy_oy * cos;
        center + egui::vec2(rx, ry)
    };

    let (off_x, off_y) = if let Some(bounds) = gc.content_bounds {
        (bounds.0, bounds.1)
    } else {
        (0.0, 0.0)
    };

    // Calculate Corners
    let p_tl = transform_point(off_x, off_y);
    let p_tr = transform_point(off_x + base_w, off_y);
    let p_br = transform_point(off_x + base_w, off_y + base_h);
    let p_bl = transform_point(off_x, off_y + base_h);

    // Midpoints (for matching handles)
    let p_t = transform_point(off_x + base_w / 2.0, off_y);

    let s_tl = to_screen(p_tl);
    let s_tr = to_screen(p_tr);
    let s_br = to_screen(p_br);
    let s_bl = to_screen(p_bl);
    let s_t = to_screen(p_t);

    let painter = ui.painter();
    let stroke = egui::Stroke::new(thickness, color);

    painter.line_segment([s_tl, s_tr], stroke);
    painter.line_segment([s_tr, s_br], stroke);
    painter.line_segment([s_br, s_bl], stroke);
    painter.line_segment([s_bl, s_tl], stroke);

    ([s_tl, s_tr, s_br, s_bl], center, angle_rad, s_t)
}
