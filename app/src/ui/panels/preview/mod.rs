use egui::Ui;
use egui_phosphor::regular as icons;
use std::sync::{Arc, RwLock};

use library::model::project::Project;
use library::EditorService;
use library::RenderServer;

use crate::command::{CommandId, CommandRegistry};
#[cfg(test)]
use crate::state::context_types::PreviewViewportRuntimeState;
#[cfg(test)]
use crate::state::context_types::SelectionTarget;
use crate::state::context_types::{PreviewPrimaryGesture, PreviewTool};
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportInputPolicy};
use crate::{action::HistoryManager, state::context::EditorContext};

mod action;
pub mod clip;
mod gizmo;
mod grid;
mod interaction;
mod qa;
mod routing;
mod support;
pub mod vector_editor;

#[cfg(test)]
use action::PreviewAction;
use qa::*;
use support::*;

pub fn preview_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    render_server: &RenderServer,
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
            available_rect.width().max(0.0),
            (available_rect.height() - bottom_bar_height - top_bar_height).max(0.0),
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
            register_preview_tool_component(
                "preview.tool.select",
                "select",
                &select_btn,
                editor_context.view.active_tool == PreviewTool::Select,
            );
            if select_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Select;
            }
            select_btn.on_hover_text("Select Tool");

            let pan_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::HAND).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Pan),
            );
            register_preview_tool_component(
                "preview.tool.pan",
                "pan",
                &pan_btn,
                editor_context.view.active_tool == PreviewTool::Pan,
            );
            if pan_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Pan;
            }
            pan_btn.on_hover_text("Pan Tool");

            let zoom_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::MAGNIFYING_GLASS).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Zoom),
            );
            register_preview_tool_component(
                "preview.tool.zoom",
                "zoom",
                &zoom_btn,
                editor_context.view.active_tool == PreviewTool::Zoom,
            );
            if zoom_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Zoom;
            }
            zoom_btn.on_hover_text("Zoom Tool");

            let text_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::TEXT_T).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Text),
            );
            register_preview_tool_component(
                "preview.tool.text",
                "text",
                &text_btn,
                editor_context.view.active_tool == PreviewTool::Text,
            );
            if text_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Text;
            }
            text_btn.on_hover_text("Text Tool");

            let shape_btn = ui.add(
                egui::Button::new(egui::RichText::new(icons::SQUARE).size(18.0))
                    .selected(editor_context.view.active_tool == PreviewTool::Shape),
            );
            register_preview_tool_component(
                "preview.tool.shape",
                "shape",
                &shape_btn,
                editor_context.view.active_tool == PreviewTool::Shape,
            );
            if shape_btn.clicked() {
                editor_context.view.active_tool = PreviewTool::Shape;
            }
            shape_btn.on_hover_text("Shape Tool");
        });
    });

    let current_composition_view = project.read().ok().and_then(|project| {
        editor_context
            .get_current_composition(&project)
            .map(|composition| (composition.id, composition.width, composition.height))
    });

    // A new/changed composition always gets a default fit. While that default
    // view remains untouched, keep it centered through dock and DPI-driven
    // logical-size changes. Once the user pans or zooms, resizing preserves
    // their chosen camera.
    update_preview_fit(
        &mut editor_context.interaction.preview_viewport,
        &mut editor_context.view.pan,
        &mut editor_context.view.zoom,
        current_composition_view,
        preview_rect,
    );

    // Viewport Controller Integration
    let hand_tool_key = registry
        .commands
        .iter()
        .find(|c| c.id == CommandId::HandTool)
        .and_then(|c| c.shortcut)
        .map(|(_, key)| key);

    // Read the momentary hand key directly: pointer gesture ownership must not
    // depend on which inspector/text widget happened to retain keyboard focus.
    let hand_tool_key_down = hand_tool_key.is_some_and(|key| ui.input(|input| input.key_down(key)));
    let pan_requested = hand_tool_key_down || editor_context.view.active_tool == PreviewTool::Pan;
    let gesture_input = ui.input(|input| PreviewGestureInput {
        primary_pressed: input.pointer.button_pressed(egui::PointerButton::Primary),
        primary_down: input.pointer.button_down(egui::PointerButton::Primary),
        primary_released: input.pointer.button_released(egui::PointerButton::Primary),
        primary_dragging: input.pointer.is_decidedly_dragging(),
        press_started_in_viewport: input
            .pointer
            .press_origin()
            .is_some_and(|position| preview_rect.contains(position)),
        pan_requested,
    });
    let gesture_decision = arbitrate_primary_gesture(
        &mut editor_context.interaction.preview_viewport.primary_gesture,
        gesture_input,
    );

    let pointer_delta = ui.input(|input| input.pointer.delta());
    let (mut viewport_changed, response) = {
        let mut state = PreviewViewportState {
            pan: &mut editor_context.view.pan,
            zoom: &mut editor_context.view.zoom,
        };
        let controller_id = ui.make_persistent_id("unique_preview_viewport_controller_id");
        let mut controller = ViewportController::new(ui, controller_id, None)
            .with_config(ViewportConfig {
                input_policy: ViewportInputPolicy::Trackpad,
                min_zoom: PREVIEW_MIN_ZOOM,
                max_zoom: PREVIEW_MAX_ZOOM,
                ..Default::default()
            })
            // A latched primary pan uses the raw per-frame pointer delta below
            // instead of asking Response to re-arbitrate gesture ownership.
            .with_pan_tool_active(!gesture_input.primary_down && pan_requested)
            .with_zoom_tool_active(
                editor_context.view.active_tool == PreviewTool::Zoom && !gesture_decision.pan_owned,
            );

        controller.interact_with_rect(
            preview_rect,
            &mut state,
            &mut editor_context.interaction.handled_hand_tool_drag,
        )
    };
    viewport_changed |= apply_owned_primary_pan(
        gesture_decision.pan_owned,
        gesture_input.primary_pressed,
        gesture_input.primary_down,
        gesture_input.primary_released,
        pointer_delta,
        &mut editor_context.view.pan,
        &mut editor_context.interaction.handled_hand_tool_drag,
    );
    if gesture_decision.pan_owned {
        ui.output_mut(|output| {
            output.cursor_icon = if gesture_input.primary_down {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            };
        });
    }

    if viewport_changed {
        editor_context.interaction.preview_viewport.auto_fit = false;
    }

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
    let mut current_interaction_visuals = Vec::new();
    let mut frame_evaluation_failed = false;
    let mut requested_frame_info = None;
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

        // Draw Frame Border
        painter.rect_stroke(
            egui::Rect::from_min_max(screen_frame_min, screen_frame_max),
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(50)), // Faint white border
            egui::StrokeKind::Middle,
        );

        // Calculate current frame and Request Render
        if let Some(comp) = editor_context.get_current_composition(&proj_read) {
            let current_frame =
                (editor_context.timeline.current_time as f64 * comp.fps).round() as u64;

            if let Some(comp_idx) = proj_read.compositions.iter().position(|c| c.id == comp.id) {
                let plugin_manager = project_service.get_plugin_manager();
                let property_evaluators = plugin_manager.get_property_evaluators();

                let render_scale = ((editor_context.view.zoom
                    * ui.ctx().pixels_per_point()
                    * editor_context.view.preview_resolution)
                    as f64)
                    .clamp(0.01, 1.0);

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
                        &plugin_manager,
                    );

                    frame_evaluation_failed =
                        !dispatch_preview_frame(frame_info, editor_context, |frame_info| {
                            requested_frame_info = Some(frame_info.clone());
                            render_server.send_request(frame_info)
                        });
                }
            }
        }

        // 2. Poll for results and update texture
        let mut latest_result = None;
        while let Ok(result) = render_server.poll_result() {
            latest_result = Some(result);
        }

        // Always drain the RenderServer, but only publish pixels evaluated from
        // the Project/time/viewport requested by this UI frame. The worker may
        // finish an older request after the user seeks or edits the Project;
        // applying that result would briefly expose stale pixels as current.
        let mut completed_current_request = false;
        if let Some(result) = latest_result.filter(|result| {
            preview_result_is_current(
                frame_evaluation_failed,
                requested_frame_info.as_ref(),
                &result.frame_info,
            )
        }) {
            completed_current_request = true;
            match result.output {
                Ok(output) => {
                    clear_preview_render_error(editor_context);
                    editor_context.preview_region = result.frame_info.region;
                    match output {
                        library::rendering::renderer::RenderOutput::Image(image) => {
                            if crate::qa::is_enabled() {
                                let (nontransparent_pixels, pixel_hash) =
                                    rgba_image_probe(&image.data);
                                editor_context.preview_nontransparent_pixels =
                                    Some(nontransparent_pixels);
                                editor_context.preview_pixel_hash = Some(pixel_hash);
                            } else {
                                editor_context.preview_nontransparent_pixels = None;
                                editor_context.preview_pixel_hash = None;
                            }
                            let size = [image.width as usize, image.height as usize];
                            let color_image =
                                egui::ColorImage::from_rgba_unmultiplied(size, &image.data);

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
                            editor_context.preview_texture = None;
                            editor_context.preview_texture_width = info.width;
                            editor_context.preview_texture_height = info.height;
                            editor_context.preview_nontransparent_pixels = None;
                            editor_context.preview_pixel_hash = None;
                        }
                    }
                    editor_context.preview_render_revision =
                        editor_context.preview_render_revision.wrapping_add(1);
                    editor_context.preview_frame_info = Some(result.frame_info);
                }
                Err(error) => {
                    report_preview_render_error(&error, editor_context);
                }
            }
        }
        if preview_render_wait_requires_repaint(
            frame_evaluation_failed,
            requested_frame_info.is_some(),
            completed_current_request,
        ) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
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
                        let Some(texture) = ValidGlPreviewTexture::new(texture_id, width, height)
                        else {
                            log::error!(
                                "Cannot draw invalid shared GL texture: id={texture_id}, size={width}x{height}"
                            );
                            return;
                        };

                        if let Some(interface) = skia_safe::gpu::gl::Interface::new_native() {
                            if let Some(mut context) =
                                skia_safe::gpu::direct_contexts::make_gl(interface, None)
                            {
                                // SAFETY: `texture` validates a non-zero GL name and positive
                                // i32 dimensions. The render server created this GL_TEXTURE_2D
                                // in the context shared with eframe, and the paint callback keeps
                                // that context current while Skia borrows (but does not own) it.
                                let backend_texture = unsafe {
                                    skia_safe::gpu::backend_textures::make_gl(
                                        (texture.width, texture.height),
                                        skia_safe::gpu::Mipmapped::No,
                                        skia_safe::gpu::gl::TextureInfo {
                                            target: eframe::glow::TEXTURE_2D,
                                            id: texture.id.get(),
                                            format: 0x8058, // GL_RGBA8
                                            protected: skia_safe::gpu::Protected::No,
                                        },
                                        "Texture",
                                    )
                                };

                                // SAFETY: eframe invokes this callback with its GL context current;
                                // DRAW_FRAMEBUFFER_BINDING is a valid scalar query in that context.
                                let raw_fbo_id = unsafe {
                                    gl.get_parameter_i32(eframe::glow::DRAW_FRAMEBUFFER_BINDING)
                                };
                                let Ok(fbo_id) = u32::try_from(raw_fbo_id) else {
                                    log::error!(
                                        "GL returned an invalid draw framebuffer binding: {raw_fbo_id}"
                                    );
                                    return;
                                };

                                let backend_render_target =
                                    skia_safe::gpu::backend_render_targets::make_gl(
                                        (texture.width, texture.height),
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

        // Interaction geometry comes from the synchronous evaluation of the
        // current Project request, never from a previously rendered frame.
        // Pixels may finish asynchronously, but stale graph provenance must
        // not be projected onto current Nodes for hit testing or mutation.
        let gui_clips = preview_frame_for_interaction(
            requested_frame_info.as_ref(),
            editor_context.preview_frame_info.as_ref(),
        )
        .map(|frame| clip::from_evaluated_frame(&proj_read, frame))
        .unwrap_or_default();
        let mut ambiguous_facade_candidates = None;
        if let Some(primary) = editor_context.selection.primary() {
            let has_matching_explicit_target = editor_context
                .interaction
                .preview_edit_target
                .as_ref()
                .is_some_and(|target| {
                    target.owner == primary
                        && routing::exact_visual_for_edit_target(&gui_clips, target).is_some()
                });
            if !has_matching_explicit_target {
                match routing::resolve_primary_edit_target(&proj_read, &gui_clips, primary) {
                    clip::OwnerEditTargetResolution::Resolved(target) => {
                        editor_context.interaction.preview_edit_target = Some(target);
                    }
                    clip::OwnerEditTargetResolution::Ambiguous { candidate_node_ids } => {
                        editor_context.interaction.preview_edit_target = None;
                        ambiguous_facade_candidates = Some(candidate_node_ids);
                    }
                    clip::OwnerEditTargetResolution::Unavailable => {
                        editor_context.interaction.preview_edit_target = None;
                    }
                }
            }
        }
        register_preview_visual_qa_components(&gui_clips, rect, &to_screen);
        if let Some(candidate_node_ids) = ambiguous_facade_candidates {
            let badge_rect = egui::Rect::from_min_size(
                rect.right_top() + egui::vec2(-260.0, 8.0),
                egui::vec2(252.0, 24.0),
            );
            ui.painter()
                .rect_filled(badge_rect, 4.0, egui::Color32::from_black_alpha(180));
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Advanced graph · click a visual to edit",
                egui::FontId::proportional(12.0),
                egui::Color32::LIGHT_GRAY,
            );
            crate::qa::register_component_with_metadata(
                "preview.facade.ambiguous",
                "preview_facade_status",
                badge_rect,
                false,
                Some(serde_json::json!({
                    "reason": "multiple independent spatial transforms",
                    "candidate_node_ids": candidate_node_ids,
                })),
            );
        }

        // Interactions
        {
            let mut interactions = interaction::PreviewInteractions::new(
                ui,
                editor_context,
                &gui_clips,
                to_screen,
                to_world,
            );
            interactions.handle(
                &response,
                rect,
                gesture_decision.pan_owned,
                &mut pending_actions,
            );
            if !gesture_decision.pan_owned {
                interactions.draw_text_overlay(&mut pending_actions);
            }
        }

        // Draw Gizmo
        if editor_context.view.active_tool == PreviewTool::Select {
            gizmo::draw_gizmo(
                ui,
                editor_context,
                &gui_clips,
                to_screen,
                !gesture_decision.pan_owned,
            );
        } else if editor_context.view.active_tool == PreviewTool::Shape {
            if let Some(state) = &editor_context.interaction.vector_editor_state {
                if let Some(edit_target) = editor_context
                    .interaction
                    .preview_edit_target
                    .as_ref()
                    .filter(|target| editor_context.selection.primary() == Some(target.owner))
                {
                    if let Some(gc) = routing::exact_visual_for_edit_target(&gui_clips, edit_target)
                    {
                        if let Some(path) = gc.content_node.properties().get_string("path") {
                            match crate::ui::panels::preview::vector_editor::svg_parser::parse_svg_path(&path) {
                                Ok(path) => {
                                    let renderer = crate::ui::panels::preview::vector_editor::renderer::VectorEditorRenderer {
                                        state,
                                        path: &path,
                                        transform: gc.world_transform,
                                        to_screen: Box::new(to_screen),
                                    };
                                    renderer.draw(ui.painter());
                                }
                                Err(error) => {
                                    log::warn!("Cannot draw invalid shape path: {error}");
                                }
                            }
                        }
                    }
                }
            }
        }
        current_interaction_visuals = gui_clips;
    } // End of project.read() scope

    // Nested gizmo/path widgets can be the first widgets to recognize a drag.
    // Record that ownership before a later Space press can claim it.
    if editor_context.interaction.preview_viewport.primary_gesture == PreviewPrimaryGesture::Pending
        && gesture_input.primary_down
        && (editor_context.interaction.gizmo_state.is_some()
            || editor_context.interaction.body_drag_state.is_some()
            || editor_context
                .interaction
                .preview_selection_drag_start
                .is_some()
            || editor_context
                .interaction
                .vector_editor_state
                .as_ref()
                .is_some_and(|state| state.selected_handle.is_some()))
    {
        editor_context.interaction.preview_viewport.primary_gesture =
            PreviewPrimaryGesture::Content;
    }

    if gesture_decision.finish_after_frame {
        editor_context.interaction.preview_viewport.primary_gesture = PreviewPrimaryGesture::Idle;
        // If Space was released before the pointer, shortcut dispatch already
        // consumed that release. Do not leave the shared suppression latch set
        // until the next unrelated Space tap.
        if !hand_tool_key_down {
            editor_context.interaction.handled_hand_tool_drag = false;
        }
    }

    // Publish the completed frame after gesture cleanup so component metadata
    // and `/v1/state` describe the same camera and owner. The helper exits
    // before allocating JSON in normal, QA-disabled builds.
    register_preview_qa_components(preview_rect, current_composition_view, editor_context);

    if apply_preview_actions(
        pending_actions,
        &current_interaction_visuals,
        project_service,
        project,
        history_manager,
    ) {
        ui.ctx().request_repaint();
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

#[cfg(test)]
include!("tests.rs");
#[cfg(test)]
include!("tests_render.rs");
