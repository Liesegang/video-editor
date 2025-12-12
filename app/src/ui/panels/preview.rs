use egui::Ui;
use std::sync::{Arc, RwLock};

use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use library::RenderServer;
use library::model::project::asset::AssetKind;

use crate::{action::HistoryManager, state::context::EditorContext};

pub fn preview_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // HistoryManager is not directly used in preview, but kept for consistency
    project_service: &ProjectService,
    project: &Arc<RwLock<Project>>,
    render_server: &Arc<RenderServer>,
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

    // Video frame outline and Preview Image
    let frame_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let screen_frame_min = to_screen(frame_rect.min);
    let screen_frame_max = to_screen(frame_rect.max);

    // Calculate current frame and Request Render
    if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            let current_frame = (editor_context.current_time as f64 * comp.fps).round() as u64;

            if let Some(comp_idx) = proj_read.compositions.iter().position(|c| c.id == comp.id) {
                let plugin_manager = project_service.get_plugin_manager();
                let property_evaluators = plugin_manager.get_property_evaluators();
                let entity_converter_registry = plugin_manager.get_entity_converter_registry();

                let _image_rect = if let Some(t) = &editor_context.preview_texture {
                    egui::vec2(t.size()[0] as f32, t.size()[1] as f32)
                } else {
                    egui::vec2(1920.0, 1080.0)
                };
                
                // Calculate scale: fit 1080p into current rect
                // We want the render to match the pixel size of the rect on screen
                let render_scale = (rect.width() / 1920.0).max(0.1).min(1.0) as f64;

                let frame_info = library::framing::get_frame_from_project(
                    &proj_read,
                    comp_idx,
                    current_frame,
                    render_scale,
                    &property_evaluators,
                    &entity_converter_registry,
                );

                render_server.send_request(frame_info);
            }
        }
    }

    // 2. Poll for results and update texture
    // Optimization: Drain queue and only process the LAST result to avoid backlog freeze/lag
    let mut latest_result = None;
    while let Ok(result) = render_server.poll_result() {
        latest_result = Some(result);
    }
    
    if let Some(result) = latest_result {
        match result.output {
            library::rendering::renderer::RenderOutput::Image(image) => {
                let size = [image.width as usize, image.height as usize];
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &image.data);

                if let Some(texture) = &mut editor_context.preview_texture {
                    texture.set(color_image, Default::default());
                } else {
                    editor_context.preview_texture = Some(ui.ctx().load_texture(
                        "preview_texture",
                        color_image,
                        Default::default(),
                    ));
                }
                editor_context.preview_texture_id = None;
            }
            library::rendering::renderer::RenderOutput::Texture(info) => {
                editor_context.preview_texture_id = Some(info.texture_id);
                editor_context.preview_texture = None; // Invalidate CPU texture
            }
        }
    }

    // 3. Draw Texture
    if let Some(texture) = &editor_context.preview_texture {
        painter.image(
            texture.id(),
            egui::Rect::from_min_max(screen_frame_min, screen_frame_max),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else if let Some(texture_id) = editor_context.preview_texture_id {
        // Zero-copy path: Draw using PaintCallback and skia (or raw GL)
        let rect = egui::Rect::from_min_max(screen_frame_min, screen_frame_max);

        let callback = egui::PaintCallback {
            rect,
            callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                move |_info, painter| {
                    use eframe::glow::HasContext;
                    let gl = painter.gl();

                    // Simple texture draw using raw GL to avoid heavy Skia context creation
                    // We assume immediate mode or cached shaders would be better, but for now simple draw.
                    // Actually, Skia is easier to write than raw GL shaders here.

                    // Note: Heavy context creation every frame. Optimization: Persistence.
                    if let Some(interface) = skia_safe::gpu::gl::Interface::new_native() {
                        if let Some(mut context) =
                            skia_safe::gpu::direct_contexts::make_gl(interface, None)
                        {
                            // Wrap the texture
                            let backend_texture = unsafe {
                                skia_safe::gpu::backend_textures::make_gl(
                                    (1920, 1080),
                                    skia_safe::gpu::Mipmapped::No,
                                    skia_safe::gpu::gl::TextureInfo {
                                        target: eframe::glow::TEXTURE_2D,
                                        id: texture_id,
                                        format: 0x8058, // GL_RGBA8
                                        protected: skia_safe::gpu::Protected::No,
                                    },
                                    "Texture",
                                )
                            };

                            // We need to draw *to* the current framebuffer (screen).
                            // Get current FBO from GL
                            let fbo_id = unsafe {
                                gl.get_parameter_i32(eframe::glow::DRAW_FRAMEBUFFER_BINDING)
                            } as u32;
                            // Get viewport
                            // _info.viewport_in_pixels();

                            // Create surface for FBO
                            // use skia_safe::gpu::backend_render_targets::make_gl for 0.82+
                            let backend_render_target =
                                skia_safe::gpu::backend_render_targets::make_gl(
                                    (1920, 1080),
                                    0, // sample count
                                    0, // stencil bits
                                    skia_safe::gpu::gl::FramebufferInfo {
                                        fboid: fbo_id,
                                        format: 0x8058, // GL_RGBA8
                                        protected: skia_safe::gpu::Protected::No,
                                    },
                                );

                            let frame_surface =
                                skia_safe::gpu::surfaces::wrap_backend_render_target(
                                    &mut context,
                                    &backend_render_target,
                                    skia_safe::gpu::SurfaceOrigin::BottomLeft,
                                    skia_safe::ColorType::RGBA8888,
                                    None,
                                    None,
                                );

                            if let Some(mut surface) = frame_surface {
                                let canvas = surface.canvas();
                                // Draw the texture image
                                // borrow_texture_from_context missing in 0.82?
                                // Workaround: Wrap backend texture as surface and snapshot (if possible)
                                if let Some(mut texture_surface) =
                                    skia_safe::gpu::surfaces::wrap_backend_texture(
                                        &mut context,
                                        &backend_texture,
                                        skia_safe::gpu::SurfaceOrigin::TopLeft,
                                        1,
                                        skia_safe::ColorType::RGBA8888,
                                        None,
                                        None,
                                    )
                                {
                                    let img = texture_surface.image_snapshot();
                                    // Draw image to fill surface
                                    canvas.draw_image(
                                        &img,
                                        (0, 0),
                                        Some(&skia_safe::Paint::default()),
                                    );
                                }
                                // Flush
                                context.flush_and_submit();
                            }
                        }
                    }
                },
            )),
        };

        ui.painter().add(callback);
    }

    let mut hovered_entity_id = None;
    let mut gui_clips: Vec<crate::model::ui_types::TimelineClip> = Vec::new();

    if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            // Collect GuiClips from current composition's tracks
            for track in &comp.tracks {
                for entity in &track.clips {
                    // Try to resolve asset ID from file_path property or similar
                    // In a real implementation this might be more robust.
                    // For now, if it has a file_path, we try to find the asset by path.
                    let asset_opt = if let Some(path) = entity.properties.get_string("file_path") {
                        proj_read.assets.iter().find(|a| a.path == path)
                    } else {
                        None
                    };

                    let asset_id = asset_opt.map(|a| a.id);
                    let asset_color = asset_opt
                        .map(|a| {
                            let c = a.color.clone();
                            egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
                        })
                        .unwrap_or(egui::Color32::GRAY);

                    let gc = crate::model::ui_types::TimelineClip {
                        id: entity.id,
                        name: entity.kind.to_string(), // entity_type -> kind
                        track_id: track.id,
                        in_frame: entity.in_frame,   // u64
                        out_frame: entity.out_frame, // u64
                        timeline_duration_frames: entity.out_frame.saturating_sub(entity.in_frame), // u64
                        source_begin_frame: entity.source_begin_frame, // u64
                        duration_frame: entity.duration_frame,         // Option<u64>
                        color: asset_color,
                        position: [
                            entity.properties.get_f32("position_x").unwrap_or(960.0),
                            entity.properties.get_f32("position_y").unwrap_or(540.0),
                        ],
                        scale_x: entity.properties.get_f32("scale_x").unwrap_or(100.0),
                        scale_y: entity.properties.get_f32("scale_y").unwrap_or(100.0),
                        anchor_x: entity.properties.get_f32("anchor_x").unwrap_or(0.0),
                        anchor_y: entity.properties.get_f32("anchor_y").unwrap_or(0.0),
                        opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                        rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                        asset_id: asset_id,
                        width: asset_opt.and_then(|a| a.width.map(|w| w as f32)),
                        height: asset_opt.and_then(|a| a.height.map(|h| h as f32)),
                    };
                    gui_clips.push(gc);
                }
            }

            // Clip hit test
            if let Some(mouse_screen_pos) = pointer_pos {
                if rect.contains(mouse_screen_pos) {
                    let mut sorted_clips: Vec<&crate::model::ui_types::TimelineClip> = gui_clips
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

                        // Check if audio
                        let is_audio = if let Some(aid) = gc.asset_id {
                            proj_read
                                .assets
                                .iter()
                                .find(|a| a.id == aid)
                                .map(|a| a.kind == AssetKind::Audio)
                                .unwrap_or(false)
                        } else {
                            false
                        };

                        if is_audio {
                            continue;
                        }

                        let base_w = gc.width.unwrap_or(1920.0);
                        let base_h = gc.height.unwrap_or(1080.0);
                        let sx = gc.scale_x / 100.0;
                        let sy = gc.scale_y / 100.0;
                        
                        // Transform point from Screen to Local
                        // World = Pos + Rot * (Local * Scale - Anchor * Scale)
                        // This seems complex to invert. Easier to check if point is in OBB.
                        
                        // Let's use the forward transform logic to define the OBB corners
                        let center_curr = egui::pos2(gc.position[0], gc.position[1]);
                        let angle_rad = gc.rotation.to_radians();
                        let cos = angle_rad.cos();
                        let sin = angle_rad.sin();
                        
                        let _transform_point = |local_x: f32, local_y: f32| -> egui::Pos2 {
                            let ox = local_x - gc.anchor_x;
                            let oy = local_y - gc.anchor_y;
                            let sx_ox = ox * sx;
                            let sy_oy = oy * sy;
                            let rx = sx_ox * cos - sy_oy * sin;
                            let ry = sx_ox * sin + sy_oy * cos;
                            center_curr + egui::vec2(rx, ry)
                        };


                        
                        // Check if mouse_world_pos is inside the quad defined by p1, p2, p3, p4
                        // Using barycentric coordinates or separating axis theorem.
                        // Or simpler: Transform mouse into local un-rotated, un-scaled space.
                        let mouse_world_pos = to_world(mouse_screen_pos);
                        let mouse_world_vec = mouse_world_pos - center_curr;
                        // Inverse Rotate
                        let inv_rx = mouse_world_vec.x * cos + mouse_world_vec.y * sin;
                        let inv_ry = -mouse_world_vec.x * sin + mouse_world_vec.y * cos;
                        
                        // Inverse Scale (Add Anchor * Scale back first? No, Scale then Anchor)
                        // Local * Scale - Anchor * Scale = Rotated
                        // Local * Scale = Rotated + Anchor * Scale
                        // Local = Rotated/Scale + Anchor
                        
                        let local_x = inv_rx / sx + gc.anchor_x;
                        let local_y = inv_ry / sy + gc.anchor_y;
                        
                        if local_x >= 0.0 && local_x <= base_w && local_y >= 0.0 && local_y <= base_h {
                             hovered_entity_id = Some(gc.id);
                             break;
                        }
                    }
                }
            }
        }
    }

    let mut interacted_with_gizmo = false;

    // Handle Gizmo Interaction (Drag)
    // Handle Gizmo Interaction (Drag)
    // Extract Gizmo Information first to avoid double borrow of editor_context
    let gizmo_drag_data = if let Some(state) = &editor_context.gizmo_state {
        Some((state.start_mouse_pos, state.active_handle, state.original_position, state.original_scale_x, state.original_scale_y, state.original_rotation, state.original_width, state.original_height, state.original_anchor_x, state.original_anchor_y))
    } else {
        None
    };

    if let Some((start_mouse_pos, active_handle, orig_pos, orig_sx, orig_sy, orig_rot, orig_w, orig_h, _orig_ax, _orig_ay)) = gizmo_drag_data {
        if ui.input(|i| i.pointer.any_released()) {
            editor_context.gizmo_state = None;
            interacted_with_gizmo = true; // Prevent click-through to selection logic on release
        } else if let Some(mouse_pos) = pointer_pos {
            interacted_with_gizmo = true;
            
            // Re-acquire selected entity data
            if let Some(selected_id) = editor_context.selected_entity_id {
                 // Clone needed properties to avoid borrow issues
                 let (comp_id, track_id, current_props) = if let Ok(proj_read) = project.read() {
                        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
                            if let Some(track) = comp.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == selected_id)) {
                                if let Some(clip) = track.clips.iter().find(|c| c.id == selected_id) {
                                    (Some(comp.id), Some(track.id), Some(clip.properties.clone()))
                                } else { (None, None, None) }
                            } else { (None, None, None) }
                        } else { (None, None, None) }
                 } else { (None, None, None) };

                 if let (Some(comp_id), Some(track_id), Some(_)) = (comp_id, track_id, current_props) {
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
                     let rotate_vec = |v: egui::Vec2, angle_deg: f32| -> egui::Vec2 {
                         let rad = angle_deg.to_radians();
                         let c = rad.cos();
                         let s = rad.sin();
                         egui::vec2(v.x * c - v.y * s, v.x * s + v.y * c)
                     };

                     match active_handle {
                         crate::model::ui_types::GizmoHandle::Rotation => {
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
                            // Convert delta to Local Space (relative to un-rotated object)
                            // We need to project the world delta onto the local axes.
                            // Local X axis: Rotated (cos, sin)
                            // Local Y axis: Rotated (-sin, cos)
                            
                            let rad = orig_rot.to_radians();
                            let c = rad.cos();
                            let s = rad.sin();
                            
                            // Delta in aligned space
                            let dx = delta_world.x * c + delta_world.y * s;
                            let dy = -delta_world.x * s + delta_world.y * c;
                            
                            // Calculate resize factor
                            // We assume default anchor (center) for logic simplicity, then compensate?
                            // No, let's just adjust scale based on edge movement.
                            // Scale = NewDimension / BaseDimension * 100.
                            // CurrentDimension = Base * Scale / 100.
                            // NewDimension = CurrentDimension + delta.
                            
                            let current_w = base_w * orig_sx / 100.0;
                            let current_h = base_h * orig_sy / 100.0;
                            
                             // Determine Handle Signs (-1, 0, 1) for X and Y axes
                             // X: -1 (Left), 1 (Right), 0 (Center/None)
                             // Y: -1 (Top), 1 (Bottom), 0 (Center/None)
                             let (sign_x, sign_y) = match active_handle {
                                 crate::model::ui_types::GizmoHandle::TopLeft => (-1.0, -1.0),
                                 crate::model::ui_types::GizmoHandle::Top => (0.0, -1.0),
                                 crate::model::ui_types::GizmoHandle::TopRight => (1.0, -1.0),
                                 crate::model::ui_types::GizmoHandle::Left => (-1.0, 0.0),
                                 crate::model::ui_types::GizmoHandle::Right => (1.0, 0.0),
                                 crate::model::ui_types::GizmoHandle::BottomLeft => (-1.0, 1.0),
                                 crate::model::ui_types::GizmoHandle::Bottom => (0.0, 1.0),
                                 crate::model::ui_types::GizmoHandle::BottomRight => (1.0, 1.0),
                                 _ => (0.0, 0.0),
                             };

                             // Calculate intended change in dimensions based on handle movement
                             // If sign is 0 (e.g. Top handle), dx contributes 0 to width change.
                             // If Center Scale (Alt), we need to double the delta because we are growing in both directions.
                             let scale_factor = if center_scale { 2.0 } else { 1.0 };
                             let raw_d_w = if sign_x != 0.0 { dx * sign_x * scale_factor } else { 0.0 };
                             let raw_d_h = if sign_y != 0.0 { dy * sign_y * scale_factor } else { 0.0 };

                             let mut next_w = current_w + raw_d_w;
                             let mut next_h = current_h + raw_d_h;

                             if keep_aspect_ratio {
                                 // Simple aspect ratio constraint
                                 let ratio = if current_h != 0.0 { current_w / current_h } else { 1.0 };
                                 
                                 // Determine dominant axis
                                 // If dragging corner, pick larger change.
                                 // If dragging side, force non-dragged axis to follow.
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
                             
                             // Calculate actual resize delta applied
                             let final_d_w = next_w - current_w;
                             let final_d_h = next_h - current_h;

                             // Update Scale
                             if base_w > 0.0 { new_scale_x = next_w / base_w * 100.0; }
                             if base_h > 0.0 { new_scale_y = next_h / base_h * 100.0; }
                             
                             if !center_scale {
                                 // Compensate position to simulate corner pinning
                                 // Shift = (Sign * Delta) / 2.0
                                 // e.g. Left Handle (SignX -1). Growing (+Delta). Shift X = -1 * Delta / 2 = -Delta/2. Matches logic.
                                 let shift_x = sign_x * final_d_w / 2.0;
                                 let shift_y = sign_y * final_d_h / 2.0;
                                 
                                 let shift = rotate_vec(egui::vec2(shift_x, shift_y), orig_rot);
                                 new_pos_x += shift.x;
                                 new_pos_y += shift.y;
                             }
                         }
                     }

                     // Apply Updates
                     // Note: We use update_clip_property to push changes.
                     // This might flood history if we track every frame?
                     // Ideally we only commit history on release. For now, direct update.
                     
                     let _ = project_service.update_clip_property(comp_id, track_id, selected_id, "scale_x", library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(new_scale_x as f64)));
                     let _ = project_service.update_clip_property(comp_id, track_id, selected_id, "scale_y", library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(new_scale_y as f64)));
                     let _ = project_service.update_clip_property(comp_id, track_id, selected_id, "position_x", library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(new_pos_x as f64)));
                     let _ = project_service.update_clip_property(comp_id, track_id, selected_id, "position_y", library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(new_pos_y as f64)));
                     let _ = project_service.update_clip_property(comp_id, track_id, selected_id, "rotation", library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(new_rotation as f64)));
                     
                 }
            }
        }
    }

    if ui.input(|i| i.pointer.any_released()) {
        editor_context.is_moving_selected_entity = false;
    }

    if !is_panning_input && !interacted_with_gizmo {
        // Allow selecting on drag start so we can move unselected items immediately
        if response.drag_started() {
            if let Some(hovered) = hovered_entity_id {
                if let Some(gc) = gui_clips.iter().find(|gc| gc.id == hovered) {
                    editor_context.select_clip(hovered, gc.track_id);
                    editor_context.is_moving_selected_entity = true; // Started drag on entity
                }
            } else {
                 editor_context.is_moving_selected_entity = false; // Started drag on background
            }
        }

        if response.clicked() {
            if let Some(hovered) = hovered_entity_id {
                if let Some(gc) = gui_clips.iter().find(|gc| gc.id == hovered) {
                    editor_context.select_clip(hovered, gc.track_id);
                }
            } else {
                 // Deselect if clicked on background
                 editor_context.selected_entity_id = None;
            }
        } else if response.dragged() {
            // Guard: Only move if we started the drag on the entity
            if editor_context.is_moving_selected_entity {
                if let Some(entity_id) = editor_context.selected_entity_id {
                let current_zoom = editor_context.view_zoom;
                if let Some(comp_id) = editor_context.selected_composition_id {
                    if let Some(track_id) = editor_context.selected_track_id {
                        // Need track_id to update entity properties
                        let world_delta = response.drag_delta() / current_zoom;

                        // Update properties via ProjectService
                        project_service
                            .update_clip_property(
                                comp_id,
                                track_id,
                                entity_id,
                                "position_x",
                                library::model::project::property::PropertyValue::Number(
                                    ordered_float::OrderedFloat(
                                        project_service
                                            .with_track_mut(comp_id, track_id, |track| {
                                                track
                                                    .clips
                                                    .iter()
                                                    .find(|e| e.id == entity_id)
                                                    .and_then(|e| {
                                                        e.properties.get_f64("position_x")
                                                    })
                                                    .unwrap_or(0.0)
                                            })
                                            .unwrap_or(0.0)
                                            + world_delta.x as f64,
                                    ),
                                ),
                            )
                            .ok(); // Handle error
                        project_service
                            .update_clip_property(
                                comp_id,
                                track_id,
                                entity_id,
                                "position_y",
                                library::model::project::property::PropertyValue::Number(
                                    ordered_float::OrderedFloat(
                                        project_service
                                            .with_track_mut(comp_id, track_id, |track| {
                                                track
                                                    .clips
                                                    .iter()
                                                    .find(|e| e.id == entity_id)
                                                    .and_then(|e| {
                                                        e.properties.get_f64("position_y")
                                                    })
                                                    .unwrap_or(0.0)
                                            })
                                            .unwrap_or(0.0)
                                            + world_delta.y as f64,
                                    ),
                                ),
                            )
                            .ok(); // Handle error
                    }
                }
            }
        }
        }
    }

    // Draw Gizmo for selected entity
    if let Some(selected_id) = editor_context.selected_entity_id {
        if let Some(gc) = gui_clips.iter().find(|gc| gc.id == selected_id) {
            let base_w = gc.width.unwrap_or(1920.0);
            let base_h = gc.height.unwrap_or(1080.0);
            let sx = gc.scale_x / 100.0;
            let sy = gc.scale_y / 100.0;

            let center = egui::pos2(gc.position[0], gc.position[1]);
            let angle_rad = gc.rotation.to_radians();
            let cos = angle_rad.cos();
            let sin = angle_rad.sin();

            let transform_point = |local_x: f32, local_y: f32| -> egui::Pos2 {
                let ox = local_x - gc.anchor_x;
                let oy = local_y - gc.anchor_y;
                let sx_ox = ox * sx;
                let sy_oy = oy * sy;
                let rx = sx_ox * cos - sy_oy * sin;
                let ry = sx_ox * sin + sy_oy * cos;
                center + egui::vec2(rx, ry)
            };

            // Calculate Corners
            let p_tl = transform_point(0.0, 0.0);
            let p_tr = transform_point(base_w, 0.0);
            let p_br = transform_point(base_w, base_h);
            let p_bl = transform_point(0.0, base_h);
            
            // Midpoints
            let p_t = transform_point(base_w / 2.0, 0.0);
            let p_b = transform_point(base_w / 2.0, base_h);
            let p_l = transform_point(0.0, base_h / 2.0);
            let p_r = transform_point(base_w, base_h / 2.0);
            
            // Rotation Handle (sticking out top)
            // Center top is p_t.
            let rot_handle_dist = 10.0 / editor_context.view_zoom; // Fixed screen distance 20px
            let s_rot = to_screen(p_t) + egui::vec2(sin * rot_handle_dist, -cos * rot_handle_dist); // Approx visual up
            // Let's use fixed screen offset logic for rotation handle drawing.
            
            // Screen Coords
            let s_tl = to_screen(p_tl);
            let s_tr = to_screen(p_tr);
            let s_br = to_screen(p_br);
            let s_bl = to_screen(p_bl);
            let s_t = to_screen(p_t);
            let s_b = to_screen(p_b);
            let s_l = to_screen(p_l);
            let s_r = to_screen(p_r);
            let s_center = to_screen(center);


            // Draw Box
            let gizmo_color = egui::Color32::from_rgb(0, 200, 255);
            let stroke = egui::Stroke::new(2.0, gizmo_color);

            painter.line_segment([s_tl, s_tr], stroke);
            painter.line_segment([s_tr, s_br], stroke);
            painter.line_segment([s_br, s_bl], stroke);
            painter.line_segment([s_bl, s_tl], stroke);
            
            // Draw Rotation Stick
            painter.line_segment([s_t, s_rot], stroke);
            painter.circle_filled(s_rot, 5.0, gizmo_color);

            // Draw Handles
            let handle_radius = 5.0;
            // Define handles structure
            let handles = [
                (s_tl, crate::model::ui_types::GizmoHandle::TopLeft, egui::CursorIcon::ResizeNwSe),
                (s_tr, crate::model::ui_types::GizmoHandle::TopRight, egui::CursorIcon::ResizeNeSw),
                (s_bl, crate::model::ui_types::GizmoHandle::BottomLeft, egui::CursorIcon::ResizeNeSw),
                (s_br, crate::model::ui_types::GizmoHandle::BottomRight, egui::CursorIcon::ResizeNwSe),
                (s_t, crate::model::ui_types::GizmoHandle::Top, egui::CursorIcon::ResizeVertical),
                (s_b, crate::model::ui_types::GizmoHandle::Bottom, egui::CursorIcon::ResizeVertical),
                (s_l, crate::model::ui_types::GizmoHandle::Left, egui::CursorIcon::ResizeHorizontal),
                (s_r, crate::model::ui_types::GizmoHandle::Right, egui::CursorIcon::ResizeHorizontal),
                (s_rot, crate::model::ui_types::GizmoHandle::Rotation, egui::CursorIcon::Grab),
            ];
            
            for (pos, handle_type, cursor) in &handles {
                 painter.circle_filled(*pos, handle_radius, egui::Color32::WHITE);
                 painter.circle_stroke(*pos, handle_radius, stroke);
                 
                 // Hit Test for Start Drag
                 if editor_context.gizmo_state.is_none() && !is_panning_input {
                     if let Some(mouse_pos) = pointer_pos {
                         if pos.distance(mouse_pos) <= handle_radius + 2.0 {
                             ui.ctx().set_cursor_icon(*cursor);
                             if ui.input(|i| i.pointer.primary_pressed()) {
                                 // Start Drag
                                 use crate::state::context::GizmoState;
                                 editor_context.gizmo_state = Some(GizmoState {
                                     start_mouse_pos: mouse_pos, // Screen space start? or World? We used world in logic.
                                     // Let's store Screen for simple delta or convert to World?
                                     // Context struct uses `start_mouse_pos: egui::Pos2`.
                                     // Drag logic used `to_world(state.start_mouse_pos)`.
                                     // So storing SCREEN pos is fine if we convert later.
                                     
                                     active_handle: *handle_type,
                                     original_position: gc.position,
                                     original_scale_x: gc.scale_x,
                                     original_scale_y: gc.scale_y,
                                     original_rotation: gc.rotation,
                                     original_anchor_x: gc.anchor_x,
                                     original_anchor_y: gc.anchor_y,
                                     original_width: base_w,
                                     original_height: base_h,
                                 });
                             }
                         }
                     }
                 }
            }

            // Draw Anchor (Pivot used for rotation/position) - this is 'center' in our logic
            painter.circle_filled(s_center, 3.0, egui::Color32::YELLOW);
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
