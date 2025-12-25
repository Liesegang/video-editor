use egui::Ui;
use egui_phosphor::regular as icons;
use std::sync::{Arc, RwLock};

use library::model::project::project::Project;
use library::EditorService;
use library::RenderServer;

use crate::command::{CommandId, CommandRegistry};
use crate::state::context_types::PreviewTool;
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};
use crate::{action::HistoryManager, state::context::EditorContext};
use library::model::project::property::Vec2;

mod action;
pub mod clip;
mod gizmo;
mod grid;
mod interaction;
pub mod vector_editor;

use action::PreviewAction;

struct PreviewViewportState<'a> {
    pan: &'a mut egui::Vec2,
    zoom: &'a mut f32,
}

impl<'a> ViewportState for PreviewViewportState<'a> {
    // Preview Pan is Translation. Positive Pan = Content Right.
    // Viewport Pan is Scroll Offset. Positive Pan (+Delta) = Content Left.
    // So we Invert.
    fn get_pan(&self) -> egui::Vec2 {
        -(*self.pan)
    }

    fn set_pan(&mut self, pan: egui::Vec2) {
        *self.pan = -pan;
    }

    fn get_zoom(&self) -> egui::Vec2 {
        egui::vec2(*self.zoom, *self.zoom)
    }

    fn set_zoom(&mut self, zoom: egui::Vec2) {
        *self.zoom = zoom.x;
    }
}

pub fn preview_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    render_server: &Arc<RenderServer>,
    registry: &CommandRegistry,
) {
    let bottom_bar_height = 24.0;
    let top_bar_height = 32.0; // Added top bar
    let available_rect = ui.available_rect_before_wrap();

    // Top Bar area
    let top_bar_rect = egui::Rect::from_min_size(
        available_rect.min,
        egui::vec2(available_rect.width(), top_bar_height),
    );

    let preview_rect = egui::Rect::from_min_size(
        egui::pos2(available_rect.min.x, available_rect.min.y + top_bar_height),
        egui::vec2(
            available_rect.width(),
            available_rect.height() - bottom_bar_height - top_bar_height,
        ),
    );
    let bottom_bar_rect = egui::Rect::from_min_max(
        egui::pos2(available_rect.min.x, preview_rect.max.y),
        available_rect.max,
    );
    let rect = preview_rect;

    // Draw Top Bar
    ui.scope_builder(egui::UiBuilder::new().max_rect(top_bar_rect), |ui| {
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 0.0);

            let select_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::CURSOR).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Select),
            );
            if select_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Select;
            }
            select_btn.on_hover_text("Select Tool");

            let pan_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::HAND).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Pan),
            );
            if pan_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Pan;
            }
            pan_btn.on_hover_text("Pan Tool");

            let zoom_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::MAGNIFYING_GLASS).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Zoom),
            );
            if zoom_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Zoom;
            }
            zoom_btn.on_hover_text("Zoom Tool");

            let text_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::TEXT_T).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Text),
            );
            if text_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Text;
            }
            text_btn.on_hover_text("Text Tool");

            let shape_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::SQUARE).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Shape),
            );
            if shape_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Shape;
            }
            shape_btn.on_hover_text("Shape Tool");
        });
    });

    // Viewport Controller Integration
    let hand_tool_key = registry
        .commands
        .iter()
        .find(|c| c.id == CommandId::HandTool)
        .and_then(|c| c.shortcut)
        .map(|(_, key)| key);

    let mut state = PreviewViewportState {
        pan: &mut editor_context.view.pan,
        zoom: &mut editor_context.view.zoom,
    };

    let mut controller = ViewportController::new(
        ui,
        ui.make_persistent_id("unique_preview_viewport_controller_id"),
        hand_tool_key,
    )
    .with_config(ViewportConfig {
        zoom_uniform: true,
        ..Default::default()
    })
    .with_pan_tool_active(editor_context.view.active_tool == PreviewTool::Pan)
    .with_zoom_tool_active(editor_context.view.active_tool == PreviewTool::Zoom);

    // Provide specific rect to controller (excluding bottom bar)
    let (_changed, response) = controller.interact_with_rect(
        preview_rect,
        &mut state,
        &mut editor_context.interaction.handled_hand_tool_drag,
    );

    let _pointer_pos = response.hover_pos();
    let is_hand_tool_active = if let Some(key) = controller.hand_tool_key {
        ui.input(|i| i.key_down(key))
    } else {
        false
    };
    let _is_panning_input = is_hand_tool_active || response.dragged_by(egui::PointerButton::Middle);

    // Legacy logic (removed lines 36-64)

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
    grid::draw_grid(
        &painter,
        rect,
        editor_context.view.pan,
        editor_context.view.zoom,
    );

    // Lock project once for reading state
    let mut pending_actions = Vec::new();
    if let Ok(proj_read) = project.read() {
        let (comp_width, comp_height) =
            if let Some(comp) = editor_context.get_current_composition(&proj_read) {
                (comp.width, comp.height)
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
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            let current_frame =
                (editor_context.timeline.current_time as f64 * comp.fps).round() as u64;

            if let Some(comp_idx) = proj_read.compositions.iter().position(|c| c.id == comp.id) {
                let plugin_manager = project_service.get_plugin_manager();
                let property_evaluators = plugin_manager.get_property_evaluators();
                let entity_converter_registry = plugin_manager.get_entity_converter_registry();

                let render_scale = ((editor_context.view.zoom
                    * ui.ctx().pixels_per_point()
                    * editor_context.view.preview_resolution)
                    as f64)
                    .max(0.01)
                    .min(1.0);

                // ROI Calculation
                let visible_min_world = to_world(rect.min);
                let visible_max_world = to_world(rect.max);

                // Intersection with composition bounds
                let comp_width = comp.width as f32;
                let comp_height = comp.height as f32;

                let region_x = visible_min_world.x.max(0.0).min(comp_width);
                let region_y = visible_min_world.y.max(0.0).min(comp_height);
                let region_right = visible_max_world.x.max(0.0).min(comp_width);
                let region_bottom = visible_max_world.y.max(0.0).min(comp_height);

                let region = if region_right > region_x && region_bottom > region_y {
                    Some(library::model::frame::frame::Region {
                        x: region_x as f64,
                        y: region_y as f64,
                        width: (region_right - region_x) as f64,
                        height: (region_bottom - region_y) as f64,
                    })
                } else {
                    // Nothing visible
                    None
                };

                if let Some(valid_region) = region {
                    let frame_info = library::framing::get_frame_from_project(
                        &proj_read,
                        comp_idx,
                        current_frame,
                        render_scale,
                        Some(valid_region),
                        &property_evaluators,
                        &entity_converter_registry,
                    );
                    render_server.send_request(frame_info);
                }
            }
        }

        // 2. Poll for results and update texture
        let mut latest_result = None;
        while let Ok(result) = render_server.poll_result() {
            latest_result = Some(result);
        }

        if let Some(result) = latest_result {
            editor_context.preview_region = result.frame_info.region;
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
                    editor_context.preview_texture_width = image.width;
                    editor_context.preview_texture_height = image.height;
                }
                library::rendering::renderer::RenderOutput::Texture(info) => {
                    editor_context.preview_texture_id = Some(info.texture_id);
                    editor_context.preview_texture = None; // Invalidate CPU texture
                    editor_context.preview_texture_width = info.width;
                    editor_context.preview_texture_height = info.height;
                }
            }
        }

        // 3. Draw Texture
        if let Some(texture) = &editor_context.preview_texture {
            // Draw CPU Texture
            let mut draw_rect = egui::Rect::from_min_max(screen_frame_min, screen_frame_max);

            if let Some(region) = &editor_context.preview_region {
                let p_min = to_screen(egui::pos2(region.x as f32, region.y as f32));
                let p_max = to_screen(egui::pos2(
                    (region.x + region.width) as f32,
                    (region.y + region.height) as f32,
                ));
                draw_rect = egui::Rect::from_min_max(p_min, p_max);
            }

            painter.image(
                texture.id(),
                draw_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else if let Some(texture_id) = editor_context.preview_texture_id {
            // Zero-copy path (GPU)
            let mut draw_rect = egui::Rect::from_min_max(screen_frame_min, screen_frame_max);

            if let Some(region) = &editor_context.preview_region {
                let p_min = to_screen(egui::pos2(region.x as f32, region.y as f32));
                let p_max = to_screen(egui::pos2(
                    (region.x + region.width) as f32,
                    (region.y + region.height) as f32,
                ));
                draw_rect = egui::Rect::from_min_max(p_min, p_max);
            }

            let width = editor_context.preview_texture_width;
            let height = editor_context.preview_texture_height;

            let callback = egui::PaintCallback {
                rect: draw_rect,
                callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                    move |_info, painter| {
                        use eframe::glow::HasContext;
                        let gl = painter.gl();

                        if let Some(interface) = skia_safe::gpu::gl::Interface::new_native() {
                            if let Some(mut context) =
                                skia_safe::gpu::direct_contexts::make_gl(interface, None)
                            {
                                let backend_texture = unsafe {
                                    skia_safe::gpu::backend_textures::make_gl(
                                        (width as i32, height as i32),
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

                                let fbo_id = unsafe {
                                    gl.get_parameter_i32(eframe::glow::DRAW_FRAMEBUFFER_BINDING)
                                } as u32;

                                let backend_render_target =
                                    skia_safe::gpu::backend_render_targets::make_gl(
                                        (width as i32, height as i32),
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
                                        canvas.draw_image(
                                            &img,
                                            (0, 0),
                                            Some(&skia_safe::Paint::default()),
                                        );
                                    }
                                    context.flush_and_submit();
                                }
                            }
                        }
                    },
                )),
            };

            ui.painter().add(callback);
        }

        let mut gui_clips: Vec<clip::PreviewClip> = Vec::new();

        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            // Collect GuiClips from current composition's tracks
            for track in &comp.tracks {
                for entity in track.clips() {
                    // Try to resolve asset ID from file_path property or similar
                    let asset_opt = if let Some(path) = entity.properties.get_string("file_path") {
                        proj_read.assets.iter().find(|a| a.path == path)
                    } else {
                        None
                    };

                    let mut width = asset_opt.and_then(|a| a.width.map(|w| w as f32));
                    let mut height = asset_opt.and_then(|a| a.height.map(|h| h as f32));
                    let mut content_point: Option<[f32; 2]> = None;

                    // If dimensions are missing (e.g. Text, Shape), calculate them
                    if width.is_none() || height.is_none() {
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};

                        let mut hasher = DefaultHasher::new();
                        entity.properties.hash(&mut hasher);
                        let hash = hasher.finish();

                        // Check cache
                        let mut cached = None;
                        if let Some((cached_hash, bounds)) = editor_context
                            .interaction
                            .bounds_cache
                            .bounds
                            .get(&entity.id)
                        {
                            if *cached_hash == hash {
                                cached = Some(*bounds);
                            }
                        }

                        if let Some((x, y, w, h)) = cached {
                            width = Some(w);
                            height = Some(h);
                            content_point = Some([x, y]);
                        } else {
                            // Calculate
                            let plugin_manager = project_service.get_plugin_manager();
                            let converter_registry = plugin_manager.get_entity_converter_registry();
                            let property_evaluators = plugin_manager.get_property_evaluators();

                            let current_frame = (editor_context.timeline.current_time as f64
                                * comp.fps)
                                .round() as u64;

                            let ctx = library::framing::entity_converters::FrameEvaluationContext {
                                composition: comp,
                                property_evaluators: &property_evaluators,
                            };

                            if let Some((x, y, w, h)) =
                                converter_registry.get_entity_bounds(&ctx, entity, current_frame)
                            {
                                width = Some(w);
                                height = Some(h);
                                content_point = Some([x, y]);
                                // Update Cache
                                editor_context
                                    .interaction
                                    .bounds_cache
                                    .bounds
                                    .insert(entity.id, (hash, (x, y, w, h)));
                            }
                        }
                    }

                    let current_frame_i64 =
                        (editor_context.timeline.current_time as f64 * comp.fps).round() as i64;
                    let delta_frames = current_frame_i64 - entity.in_frame as i64;
                    let time_offset = delta_frames as f64 / comp.fps;
                    let source_start_time = entity.source_begin_frame as f64 / entity.fps;
                    let local_time = source_start_time + time_offset;

                    // Log Gizmo Time Calculation (throttle slightly if possible, or just spam per user request)
                    if editor_context.timeline.current_time.fract() < 0.1 {
                        log::info!(
                            "[Gizmo] Entity: {} | CurrentFrame: {} | LocalTime: {:.4}",
                            entity.id,
                            current_frame_i64,
                            local_time
                        );
                    }

                    let get_val = |key: &str, default: f32| {
                        entity
                            .properties
                            .get(key)
                            .map(|p| {
                                project_service.evaluate_property_value(
                                    p,
                                    &entity.properties,
                                    local_time,
                                    comp.fps,
                                )
                            })
                            .and_then(|pv| pv.get_as::<f32>())
                            .unwrap_or(default)
                    };

                    let get_vec2 = |key: &str, default: [f32; 2]| {
                        entity
                            .properties
                            .get(key)
                            .map(|p| {
                                let val = project_service.evaluate_property_value(
                                    p,
                                    &entity.properties,
                                    local_time,
                                    comp.fps,
                                );
                                val.get_as::<Vec2>()
                                    .map(|v| [v.x.into_inner() as f32, v.y.into_inner() as f32])
                                    .unwrap_or(default)
                            })
                            .unwrap_or(default)
                    };

                    let position = get_vec2("position", [960.0, 540.0]);
                    let scale = get_vec2("scale", [100.0, 100.0]);
                    let anchor = get_vec2("anchor", [0.0, 0.0]);
                    let rotation = get_val("rotation", 0.0);
                    let opacity = get_val("opacity", 100.0);

                    let transform = library::model::frame::transform::Transform {
                        position: library::model::frame::transform::Position {
                            x: position[0] as f64,
                            y: position[1] as f64,
                        },
                        scale: library::model::frame::transform::Scale {
                            x: scale[0] as f64,
                            y: scale[1] as f64,
                        },
                        rotation: rotation as f64,
                        anchor: library::model::frame::transform::Position {
                            x: anchor[0] as f64,
                            y: anchor[1] as f64,
                        },
                        opacity: opacity as f64,
                    };

                    let content_bounds = if let (Some(w), Some(h)) = (width, height) {
                        let (cx, cy) = if let Some(pt) = content_point {
                            (pt[0], pt[1])
                        } else {
                            (0.0, 0.0)
                        };
                        Some((cx, cy, w, h))
                    } else {
                        None
                    };

                    let gc = clip::PreviewClip {
                        clip: entity,
                        track_id: track.id,
                        transform,
                        content_bounds,
                    };
                    gui_clips.push(gc);
                }
            }
        }

        // Interactions
        {
            let mut interactions = interaction::PreviewInteractions::new(
                ui,
                editor_context,
                &project,
                history_manager,
                &gui_clips,
                to_screen,
                to_world,
            );
            interactions.handle(&response, rect, &mut pending_actions);
            interactions.draw_text_overlay(&mut pending_actions);
        }

        // Draw Gizmo
        if editor_context.view.active_tool == PreviewTool::Select {
            gizmo::draw_gizmo(ui, editor_context, &gui_clips, to_screen);
        } else if editor_context.view.active_tool == PreviewTool::Shape {
            if let Some(state) = &editor_context.interaction.vector_editor_state {
                if let Some(id) = editor_context.selection.selected_entities.iter().next() {
                    if let Some(gc) = gui_clips.iter().find(|c| c.id() == *id) {
                        let renderer = crate::ui::panels::preview::vector_editor::renderer::VectorEditorRenderer {
                            state,
                            transform: gc.transform.clone(),
                            to_screen: Box::new(|p| to_screen(p)),
                        };
                        renderer.draw(ui.painter());
                    }
                }
            }
        }
    } // End of project.read() scope

    // Execute pending actions
    for action in pending_actions {
        match action {
            PreviewAction::UpdateProperty {
                comp_id,
                track_id,
                entity_id,
                prop_name,
                time,
                value,
            } => {
                crate::utils::property::update_property(
                    project_service,
                    comp_id,
                    track_id,
                    entity_id,
                    &prop_name,
                    time,
                    value,
                );
            }
        }
    }

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
