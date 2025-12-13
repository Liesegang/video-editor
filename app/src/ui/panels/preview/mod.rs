use egui::Ui;
use std::sync::{Arc, RwLock};

use library::model::project::asset::AssetKind;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use library::RenderServer;

use crate::{action::HistoryManager, state::context::EditorContext};

mod gizmo;
mod grid;

pub fn preview_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // HistoryManager is not directly used in preview, but kept for consistency
    project_service: &ProjectService,
    project: &Arc<RwLock<Project>>,
    render_server: &Arc<RenderServer>,
) {
    let bottom_bar_height = 24.0;
    let available_rect = ui.available_rect_before_wrap();
    let preview_rect = egui::Rect::from_min_size(
        available_rect.min,
        egui::vec2(
            available_rect.width(),
            available_rect.height() - bottom_bar_height,
        ),
    );
    let bottom_bar_rect = egui::Rect::from_min_max(
        egui::pos2(available_rect.min.x, preview_rect.max.y),
        available_rect.max,
    );

    let response = ui.allocate_rect(preview_rect, egui::Sense::click_and_drag());
    let rect = preview_rect;

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let space_down = ui.input(|i| i.key_down(egui::Key::Space));
    let middle_down = ui
        .ctx()
        .input(|i| i.pointer.button_down(egui::PointerButton::Middle));
    let is_panning_input = space_down || middle_down;

    if is_panning_input && response.dragged() {
        editor_context.view.pan += response.drag_delta();
    }

    if response.hovered() {
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_delta != 0.0 {
            let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
            let old_zoom = editor_context.view.zoom;
            editor_context.view.zoom *= zoom_factor;

            if let Some(mouse_pos) = pointer_pos {
                let mouse_in_canvas = mouse_pos - rect.min;
                editor_context.view.pan = mouse_in_canvas
                    - (mouse_in_canvas - editor_context.view.pan)
                        * (editor_context.view.zoom / old_zoom);
            }
        }
    }

    let view_offset = rect.min + editor_context.view.pan;
    let view_zoom = editor_context.view.zoom;

    let to_screen = |pos: egui::Pos2| -> egui::Pos2 { view_offset + (pos.to_vec2() * view_zoom) };
    let to_world = |pos: egui::Pos2| -> egui::Pos2 {
        let vec = pos - view_offset;
        egui::pos2(vec.x / view_zoom, vec.y / view_zoom)
    };

    let painter = ui.painter().with_clip_rect(rect);

    // Background fill
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));

    // Grid
    // Grid
    grid::draw_grid(
        &painter,
        rect,
        editor_context.view.pan,
        editor_context.view.zoom,
    );

    // Video frame outline and Preview Image
    let (comp_width, comp_height) = if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            (comp.width, comp.height)
        } else {
            (1920, 1080)
        }
    } else {
        (1920, 1080)
    };

    let frame_rect = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(comp_width as f32, comp_height as f32),
    );
    let screen_frame_min = to_screen(frame_rect.min);
    let screen_frame_max = to_screen(frame_rect.max);

    // Calculate current frame and Request Render
    if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            let current_frame =
                (editor_context.timeline.current_time as f64 * comp.fps).round() as u64;

            if let Some(comp_idx) = proj_read.compositions.iter().position(|c| c.id == comp.id) {
                let plugin_manager = project_service.get_plugin_manager();
                let property_evaluators = plugin_manager.get_property_evaluators();
                let entity_converter_registry = plugin_manager.get_entity_converter_registry();

                let _image_rect = if let Some(t) = &editor_context.preview_texture {
                    egui::vec2(t.size()[0] as f32, t.size()[1] as f32)
                } else {
                    egui::vec2(comp_width as f32, comp_height as f32)
                };

                let render_scale = ((editor_context.view.zoom
                    * ui.ctx().pixels_per_point()
                    * editor_context.view.preview_resolution)
                    as f64)
                    .max(0.01)
                    .min(1.0);

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
                                    (comp_width as i32, comp_height as i32),
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
                                    (comp_width as i32, comp_height as i32),
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
                            let current_frame = (editor_context.timeline.current_time as f64
                                * comp.fps)
                                .round() as u64; // Convert current_time (f32) to frame (u64)
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

                        if local_x >= 0.0
                            && local_x <= base_w
                            && local_y >= 0.0
                            && local_y <= base_h
                        {
                            hovered_entity_id = Some(gc.id);
                            break;
                        }
                    }
                }
            }
        }
    }

    // Handle Gizmo Interaction
    let interacted_with_gizmo_from_logic = gizmo::handle_gizmo_interaction(
        ui,
        editor_context,
        project,
        project_service,
        pointer_pos,
        to_world,
    );

    let interacted_with_gizmo = interacted_with_gizmo_from_logic;

    if ui.input(|i| i.pointer.any_released()) {
        editor_context.interaction.is_moving_selected_entity = false;
        editor_context.interaction.body_drag_state = None;
    }

    if !is_panning_input && !interacted_with_gizmo {
        // Allow selecting on drag start so we can move unselected items immediately
        if response.drag_started() {
            if let Some(hovered) = hovered_entity_id {
                if let Some(gc) = gui_clips.iter().find(|gc| gc.id == hovered) {
                    editor_context.select_clip(hovered, gc.track_id);
                    editor_context.interaction.is_moving_selected_entity = true;
                    // Started drag on entity - Capture State
                    if let Some(pointer_pos) = pointer_pos {
                        editor_context.interaction.body_drag_state =
                            Some(crate::state::context_types::BodyDragState {
                                start_mouse_pos: pointer_pos,
                                original_position: gc.position,
                            });
                    }
                }
            } else {
                editor_context.interaction.is_moving_selected_entity = false; // Started drag on background
                editor_context.interaction.body_drag_state = None;
            }
        }

        if response.clicked() {
            if let Some(hovered) = hovered_entity_id {
                if let Some(gc) = gui_clips.iter().find(|gc| gc.id == hovered) {
                    editor_context.select_clip(hovered, gc.track_id);
                }
            } else {
                // Deselect if clicked on background
                editor_context.selection.entity_id = None;
            }
        } else if response.dragged() {
            // Guard: Only move if we started the drag on the entity AND we have state
            if editor_context.interaction.is_moving_selected_entity {
                if let Some(entity_id) = editor_context.selection.entity_id {
                    let current_zoom = editor_context.view.zoom;
                    if let Some(comp_id) = editor_context.selection.composition_id {
                        if let Some(track_id) = editor_context.selection.track_id {
                            if let Some(drag_state) = &editor_context.interaction.body_drag_state {
                                if let Some(curr_mouse) = pointer_pos {
                                    let screen_delta = curr_mouse - drag_state.start_mouse_pos;
                                    let world_delta = screen_delta / current_zoom;

                                    let new_x = drag_state.original_position[0] as f64
                                        + world_delta.x as f64;
                                    let new_y = drag_state.original_position[1] as f64
                                        + world_delta.y as f64;

                                    let current_time = editor_context.timeline.current_time as f64;

                                    let _ = project_service.update_property_or_keyframe(
                                        comp_id,
                                        track_id,
                                        entity_id,
                                        "position_x",
                                        current_time,
                                        library::model::project::property::PropertyValue::Number(
                                            ordered_float::OrderedFloat(new_x),
                                        ),
                                        None,
                                    );

                                    let _ = project_service.update_property_or_keyframe(
                                        comp_id,
                                        track_id,
                                        entity_id,
                                        "position_y",
                                        current_time,
                                        library::model::project::property::PropertyValue::Number(
                                            ordered_float::OrderedFloat(new_y),
                                        ),
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Draw Gizmo
    gizmo::draw_gizmo(ui, editor_context, &gui_clips, to_screen);

    // Info text
    let info_text = format!(
        "Time: {:.2}\nZoom: {:.0}%",
        editor_context.timeline.current_time,
        editor_context.view.zoom * 100.0
    );
    painter.text(
        rect.left_top() + egui::vec2(10.0, 10.0),
        egui::Align2::LEFT_TOP,
        info_text,
        egui::FontId::monospace(14.0),
        egui::Color32::WHITE,
    );

    // Draw Bottom Bar
    ui.scope_builder(egui::UiBuilder::new().max_rect(bottom_bar_rect), |ui| {
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            egui::ComboBox::from_id_salt("preview_resolution")
                .selected_text(format!(
                    "{}%",
                    (editor_context.view.preview_resolution * 100.0) as i32
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor_context.view.preview_resolution, 1.0, "Full");
                    ui.selectable_value(&mut editor_context.view.preview_resolution, 0.75, "3/4");
                    ui.selectable_value(&mut editor_context.view.preview_resolution, 0.5, "1/2");
                    ui.selectable_value(&mut editor_context.view.preview_resolution, 0.25, "1/4");
                });
        });
    });
}
