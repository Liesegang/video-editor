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

mod gizmo;
mod grid;
mod interaction;
pub mod vector_editor;

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

    let mut gui_clips: Vec<crate::model::ui_types::TimelineClip> = Vec::new();

    if let Ok(proj_read) = project.read() {
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            // Collect GuiClips from current composition's tracks
            for track in &comp.tracks {
                for entity in &track.clips {
                    // Try to resolve asset ID from file_path property or similar
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

                    let get_val = |key: &str, default: f32| {
                        entity
                            .properties
                            .get(key)
                            .map(|p| {
                                project_service.evaluate_property_value(
                                    p,
                                    &entity.properties,
                                    editor_context.timeline.current_time as f64,
                                    comp.fps,
                                )
                            })
                            .and_then(|pv| pv.get_as::<f32>())
                            .unwrap_or(default)
                    };

                    let get_vec2 = |key: &str, default: [f32; 2]| {
                        entity.properties.get(key)
                            .map(|p| {
                                let val = project_service.evaluate_property_value(
                                    p,
                                    &entity.properties,
                                    editor_context.timeline.current_time as f64,
                                    comp.fps,
                                );
                                val.get_as::<Vec2>()
                                   .map(|v| [v.x.into_inner() as f32, v.y.into_inner() as f32])
                                   .unwrap_or(default)
                            })
                            .unwrap_or(default)
                    };

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
                        position: get_vec2("position", [960.0, 540.0]),
                        scale: get_vec2("scale", [100.0, 100.0]),
                        anchor: get_vec2("anchor", [0.0, 0.0]),
                        opacity: get_val("opacity", 100.0),
                        rotation: get_val("rotation", 0.0),
                        asset_id: asset_id,
                        width: width,
                        height: height,
                        content_point: content_point,
                        kind: entity.kind.clone(),
                    };
                    gui_clips.push(gc);
                }
            }
        }
    }

    // Interactions
    {
        let mut interactions = interaction::PreviewInteractions::new(
            ui,
            editor_context,
            &project,
            project_service,
            history_manager,
            &gui_clips,
            to_screen,
            to_world,
        );
        interactions.handle(&response, rect);
        interactions.draw_text_overlay();
    }

    // Draw Gizmo
    if editor_context.view.active_tool == PreviewTool::Select {
        gizmo::draw_gizmo(ui, editor_context, &gui_clips, to_screen);
    } else if editor_context.view.active_tool == PreviewTool::Shape {
        if let Some(state) = &editor_context.interaction.vector_editor_state {
            if let Some(id) = editor_context.selection.selected_entities.iter().next() {
                if let Some(gc) = gui_clips.iter().find(|c| c.id == *id) {
                    let transform = library::model::frame::transform::Transform {
                        position: library::model::frame::transform::Position {
                            x: gc.position[0] as f64,
                            y: gc.position[1] as f64,
                        },
                        scale: library::model::frame::transform::Scale {
                            x: gc.scale[0] as f64,
                            y: gc.scale[1] as f64,
                        },
                        rotation: gc.rotation as f64,
                        anchor: library::model::frame::transform::Position {
                            x: gc.anchor[0] as f64,
                            y: gc.anchor[1] as f64,
                        },
                        opacity: gc.opacity as f64,
                    };

                    let renderer =
                        crate::ui::panels::preview::vector_editor::renderer::VectorEditorRenderer {
                            state,
                            transform,
                            to_screen: Box::new(|p| to_screen(p)),
                        };
                    renderer.draw(ui.painter());
                }
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
