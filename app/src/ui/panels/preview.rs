use egui::{epaint::StrokeKind, Ui};
use std::sync::{Arc, RwLock};

use library::model::project::project::Project;
use library::service::project_service::ProjectService;

use crate::{action::HistoryManager, model::assets::AssetKind, state::context::EditorContext};

pub fn preview_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // HistoryManager is not directly used in preview, but kept for consistency
    project_service: &ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let space_down = ui.input(|i| i.key_down(egui::Key::Space));
    let middle_down = ui
        .ctx()
        .input(|i| i.pointer.button_down(egui::PointerButton::Middle));
    let is_panning_input = space_down || middle_down;

    if is_panning_input && response.dragged() {
        editor_context.view_pan += response.drag_delta();
    }

    if response.hovered() {
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_delta != 0.0 {
            let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
            let old_zoom = editor_context.view_zoom;
            editor_context.view_zoom *= zoom_factor;

            if let Some(mouse_pos) = pointer_pos {
                let mouse_in_canvas = mouse_pos - rect.min;
                editor_context.view_pan = mouse_in_canvas
                    - (mouse_in_canvas - editor_context.view_pan)
                        * (editor_context.view_zoom / old_zoom);
            }
        }
    }

    let view_offset = rect.min + editor_context.view_pan;
    let view_zoom = editor_context.view_zoom;

    let to_screen = |pos: egui::Pos2| -> egui::Pos2 { view_offset + (pos.to_vec2() * view_zoom) };
    let to_world = |pos: egui::Pos2| -> egui::Pos2 {
        let vec = pos - view_offset;
        egui::pos2(vec.x / view_zoom, vec.y / view_zoom)
    };

    let painter = ui.painter().with_clip_rect(rect);

    // Background fill
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));

    // Grid
    let grid_size = 100.0 * editor_context.view_zoom;

    if grid_size > 10.0 {
        let (_cols, _rows) = (
            (rect.width() / grid_size).ceil() as usize + 2,
            (rect.height() / grid_size).ceil() as usize + 2,
        );
        let start_x =
            rect.min.x + ((editor_context.view_pan.x % grid_size) + grid_size) % grid_size;
        let start_y =
            rect.min.y + ((editor_context.view_pan.y % grid_size) + grid_size) % grid_size;
        let grid_color = egui::Color32::from_gray(50);

        // Calculate the first visible line's coordinate for x and y
        let first_visible_x = ((rect.min.x - start_x) / grid_size).floor();
        let first_visible_y = ((rect.min.y - start_y) / grid_size).floor();

        // Draw vertical lines
        for i in (first_visible_x as i32)..=((rect.max.x - start_x) / grid_size).ceil() as i32 {
            let x = start_x + (i as f32) * grid_size;
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(1.0, grid_color),
            );
        }

        // Draw horizontal lines
        for i in (first_visible_y as i32)..=((rect.max.y - start_y) / grid_size).ceil() as i32 {
            let y = start_y + (i as f32) * grid_size;
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                egui::Stroke::new(1.0, grid_color),
            );
        }
    }

    // Video frame outline
    let frame_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let screen_frame_min = to_screen(frame_rect.min);
    let screen_frame_max = to_screen(frame_rect.max);
    painter.rect_stroke(
        egui::Rect::from_min_max(screen_frame_min, screen_frame_max),
        0.0,
        egui::Stroke::new(
            2.0 * editor_context.view_zoom.max(1.0),
            egui::Color32::WHITE,
        ),
        StrokeKind::Middle,
    );

    let mut hovered_entity_id = None;
    let mut gui_clips: Vec<crate::model::ui_types::GuiClip> = Vec::new();

    if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            // Collect GuiClips from current composition's tracks
            for track in &comp.tracks {
                for entity in &track.entities {
                    // For simplicity, hardcode asset_index 0 (first asset) for now.
                    // In a real app, this would be determined by entity_type or asset property.
                    let asset_index = 0;
                    let asset = editor_context.assets.get(asset_index);

                    if let Some(a) = asset {
                        let gc = crate::model::ui_types::GuiClip {
                            id: entity.id,
                            name: entity.entity_type.clone(), // Use entity_type as name for now
                            track_id: track.id,
                            in_frame: entity.in_frame,   // u64
                            out_frame: entity.out_frame, // u64
                            timeline_duration_frames: entity.out_frame.saturating_sub(entity.in_frame), // u64
                            source_begin_frame: entity.source_begin_frame, // u64
                            duration_frame: entity.duration_frame, // Option<u64>
                            color: a.color,
                            position: [
                                entity.properties.get_f32("position_x").unwrap_or(960.0),
                                entity.properties.get_f32("position_y").unwrap_or(540.0),
                            ],
                            scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                            opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                            rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                            asset_index,
                        };
                        gui_clips.push(gc);
                    }
                }
            }

            // Clip hit test
            if let Some(mouse_screen_pos) = pointer_pos {
                if rect.contains(mouse_screen_pos) {
                    let mut sorted_clips: Vec<&crate::model::ui_types::GuiClip> = gui_clips
                        .iter()
                        .filter(|gc| {
                            let current_frame =
                                (editor_context.current_time as f64 * comp.fps).round() as u64; // Convert current_time (f32) to frame (u64)
                            current_frame >= gc.in_frame && current_frame < gc.out_frame
                        })
                        .collect();
                    // Sort by track index for consistent Z-order hit testing
                    sorted_clips.sort_by_key(|gc| {
                        comp.tracks
                            .iter()
                            .position(|t| t.id == gc.track_id)
                            .unwrap_or(0)
                    });

                    for gc in sorted_clips.iter().rev() {
                        // Iterate in reverse to hit top-most clips first
                        let is_audio = editor_context
                            .assets
                            .get(gc.asset_index)
                            .map(|a| a.kind == AssetKind::Audio)
                            .unwrap_or(false);
                        if is_audio {
                            continue;
                        }

                        let mouse_world_pos = to_world(mouse_screen_pos);
                        let center = egui::pos2(gc.position[0], gc.position[1]);

                        let vec = mouse_world_pos - center;
                        let angle_rad = -gc.rotation.to_radians();
                        let cos = angle_rad.cos();
                        let sin = angle_rad.sin();
                        let local_x = vec.x * cos - vec.y * sin;
                        let local_y = vec.x * sin + vec.y * cos;

                        let base_w = 640.0;
                        let base_h = 360.0;
                        let half_w = (base_w * gc.scale / 100.0) / 2.0;
                        let half_h = (base_h * gc.scale / 100.0) / 2.0;

                        if local_x.abs() <= half_w && local_y.abs() <= half_h {
                            hovered_entity_id = Some(gc.id);
                            break;
                        }
                    }
                }
            }
        }
    }

    // Clip drawing
    if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            // Re-collect visible GuiClips after potential modifications in Inspector
            let mut visible_clips: Vec<crate::model::ui_types::GuiClip> = Vec::new();
            for track in &comp.tracks {
                for entity in &track.entities {
                    let asset_index = 0; // Temporary: should derive from entity properties
                    let asset = editor_context.assets.get(asset_index);

                    let current_frame =
                        (editor_context.current_time as f64 * comp.fps).round() as u64; // Convert current_time (f32) to frame (u64)
                    if current_frame >= entity.in_frame && current_frame < entity.out_frame {
                        if let Some(a) = asset {
                            let gc = crate::model::ui_types::GuiClip {
                                id: entity.id,
                                name: entity.entity_type.clone(),
                                track_id: track.id,
                                in_frame: entity.in_frame,   // u64
                                out_frame: entity.out_frame, // u64
                                timeline_duration_frames: entity.out_frame.saturating_sub(entity.in_frame), // u64
                                source_begin_frame: entity.source_begin_frame, // u64
                                duration_frame: entity.duration_frame, // Option<u64>
                                color: a.color,
                                position: [
                                    entity.properties.get_f32("position_x").unwrap_or(960.0),
                                    entity.properties.get_f32("position_y").unwrap_or(540.0),
                                ],
                                scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                                opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                                rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                                asset_index,
                            };
                            visible_clips.push(gc);
                        }
                    }
                }
            }
            // Sort by track index for consistent Z-order drawing
            visible_clips.sort_by_key(|gc| {
                comp.tracks
                    .iter()
                    .position(|t| t.id == gc.track_id)
                    .unwrap_or(0)
            });

            for clip in visible_clips {
                let is_audio = editor_context
                    .assets
                    .get(clip.asset_index)
                    .map(|a| a.kind == AssetKind::Audio)
                    .unwrap_or(false);
                if is_audio {
                    continue;
                }

                let opacity = (clip.opacity / 100.0).clamp(0.0, 1.0);
                let color = clip.color.linear_multiply(opacity);

                let base_w = 640.0;
                let base_h = 360.0;
                let scale = clip.scale / 100.0;
                let w = base_w * scale;
                let h = base_h * scale;

                let center = egui::pos2(clip.position[0], clip.position[1]);
                let rot = clip.rotation.to_radians();
                let cos_r = rot.cos();
                let sin_r = rot.sin();

                let local_corners = [
                    egui::vec2(-w / 2.0, -h / 2.0),
                    egui::vec2(w / 2.0, -h / 2.0),
                    egui::vec2(w / 2.0, h / 2.0),
                    egui::vec2(-w / 2.0, h / 2.0),
                ];

                let screen_points: Vec<egui::Pos2> = local_corners
                    .iter()
                    .map(|corner| {
                        let rot_x = corner.x * cos_r - corner.y * sin_r;
                        let rot_y = corner.x * sin_r + corner.y * cos_r;
                        to_screen(center + egui::vec2(rot_x, rot_y))
                    })
                    .collect();

                painter.add(egui::Shape::convex_polygon(
                    screen_points.clone(),
                    color,
                    egui::Stroke::NONE,
                ));
            }
        }
    }

    let interacted_with_gizmo = false; // Placeholder

    if !is_panning_input && !interacted_with_gizmo {
        if response.clicked() {
            editor_context.selected_entity_id = hovered_entity_id;
        } else if response.dragged() {
            if let Some(entity_id) = editor_context.selected_entity_id {
                let current_zoom = editor_context.view_zoom;
                if let Some(comp_id) = editor_context.selected_composition_id {
                    if let Some(track_id) = editor_context.selected_track_id {
                        // Need track_id to update entity properties
                        let world_delta = response.drag_delta() / current_zoom;

                        // Update properties via ProjectService
                        project_service
                            .update_entity_property(
                                comp_id,
                                track_id,
                                entity_id,
                                "position_x",
                                library::model::project::property::PropertyValue::Number(
                                    project_service
                                        .with_track_mut(comp_id, track_id, |track| {
                                            track
                                                .entities
                                                .iter()
                                                .find(|e| e.id == entity_id)
                                                .and_then(|e| e.properties.get_f64("position_x"))
                                                .unwrap_or(0.0)
                                        })
                                        .unwrap_or(0.0)
                                        + world_delta.x as f64,
                                ),
                            )
                            .ok(); // Handle error
                        project_service
                            .update_entity_property(
                                comp_id,
                                track_id,
                                entity_id,
                                "position_y",
                                library::model::project::property::PropertyValue::Number(
                                    project_service
                                        .with_track_mut(comp_id, track_id, |track| {
                                            track
                                                .entities
                                                .iter()
                                                .find(|e| e.id == entity_id)
                                                .and_then(|e| e.properties.get_f64("position_y"))
                                                .unwrap_or(0.0)
                                        })
                                        .unwrap_or(0.0)
                                        + world_delta.y as f64,
                                ),
                            )
                            .ok(); // Handle error
                    }
                }
            }
        }
    }

    // Info text
    let info_text = format!(
        "Time: {:.2}\nZoom: {:.0}%",
        editor_context.current_time,
        editor_context.view_zoom * 100.0
    );
    painter.text(
        rect.left_top() + egui::vec2(10.0, 10.0),
        egui::Align2::LEFT_TOP,
        info_text,
        egui::FontId::monospace(14.0),
        egui::Color32::WHITE,
    );
}
