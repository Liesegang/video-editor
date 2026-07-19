use egui::Ui;
use egui_phosphor::regular as icons;
use std::sync::{Arc, RwLock};

use library::model::asset::AssetKind; // Added Import
use library::model::project::Project;
use library::EditorService;
use library::RenderServer;

use crate::command::{CommandId, CommandRegistry};
use crate::state::context_types::{PreviewPrimaryGesture, PreviewTool};
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};
use crate::{action::HistoryManager, state::context::EditorContext};
use library::model::property::Vec2;

mod action;
pub mod clip;
mod gizmo;
mod grid;
mod interaction;
pub mod vector_editor;

use action::PreviewAction;

const PREVIEW_FIT_PADDING: f32 = 24.0;
const PREVIEW_MIN_ZOOM: f32 = 0.0001;
const PREVIEW_MAX_ZOOM: f32 = 1000.0;

fn rgba_image_probe(data: &[u8]) -> (u64, u64) {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    let nontransparent_pixels = data.chunks_exact(4).filter(|pixel| pixel[3] != 0).count() as u64;
    (nontransparent_pixels, hash)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FittedPreviewView {
    /// Translation relative to the Preview rect's minimum corner.
    pan: egui::Vec2,
    zoom: f32,
}

/// Fit a composition canvas into the actual Preview allocation.
///
/// egui rectangles and pointer coordinates are expressed in logical points,
/// so pixels-per-point intentionally does not participate in this geometry.
/// It is applied later only when choosing the renderer's pixel resolution.
fn fit_canvas_to_viewport(
    viewport_rect: egui::Rect,
    canvas_size: egui::Vec2,
) -> Option<FittedPreviewView> {
    let viewport_size = viewport_rect.size();
    if !viewport_size.x.is_finite()
        || !viewport_size.y.is_finite()
        || !canvas_size.x.is_finite()
        || !canvas_size.y.is_finite()
        || viewport_size.x <= 0.0
        || viewport_size.y <= 0.0
        || canvas_size.x <= 0.0
        || canvas_size.y <= 0.0
    {
        return None;
    }

    // Keep the margin useful in normal panels without letting it consume a
    // tiny allocation after a dock resize.
    let padding_x = PREVIEW_FIT_PADDING.min(viewport_size.x * 0.1);
    let padding_y = PREVIEW_FIT_PADDING.min(viewport_size.y * 0.1);
    let available = egui::vec2(
        (viewport_size.x - padding_x * 2.0).max(f32::EPSILON),
        (viewport_size.y - padding_y * 2.0).max(f32::EPSILON),
    );
    let zoom = (available.x / canvas_size.x)
        .min(available.y / canvas_size.y)
        .clamp(PREVIEW_MIN_ZOOM, PREVIEW_MAX_ZOOM);
    let pan = (viewport_size - canvas_size * zoom) * 0.5;

    Some(FittedPreviewView { pan, zoom })
}

#[derive(Clone, Copy, Debug, Default)]
struct PreviewGestureInput {
    primary_pressed: bool,
    primary_down: bool,
    primary_released: bool,
    primary_dragging: bool,
    press_started_in_viewport: bool,
    pan_requested: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PreviewGestureDecision {
    pan_owned: bool,
    finish_after_frame: bool,
}

/// Arbitrate the primary pointer once per press, then retain that owner until
/// release. Modifier changes cannot leak one physical gesture into two tools.
fn arbitrate_primary_gesture(
    owner: &mut PreviewPrimaryGesture,
    input: PreviewGestureInput,
) -> PreviewGestureDecision {
    if *owner == PreviewPrimaryGesture::Idle
        && input.primary_pressed
        && input.press_started_in_viewport
    {
        *owner = if input.pan_requested {
            PreviewPrimaryGesture::Pan
        } else {
            PreviewPrimaryGesture::Pending
        };
    }

    if *owner == PreviewPrimaryGesture::Pending && input.primary_down {
        if input.pan_requested {
            // Space may be pressed after the pointer, provided no content drag
            // has actually started yet.
            *owner = PreviewPrimaryGesture::Pan;
        } else if input.primary_dragging {
            *owner = PreviewPrimaryGesture::Content;
        }
    }

    let pan_owned = *owner == PreviewPrimaryGesture::Pan;
    let finish_after_frame = *owner != PreviewPrimaryGesture::Idle
        && (input.primary_released || (!input.primary_down && !input.primary_pressed));

    PreviewGestureDecision {
        pan_owned,
        finish_after_frame,
    }
}

/// Submit only a fully evaluated frame. Evaluation failures invalidate the
/// displayed output because keeping a previous texture would present stale
/// pixels as if they were the current Project state.
const PREVIEW_EVALUATION_ERROR_PREFIX: &str = "Failed to evaluate preview frame: ";
const PREVIEW_RENDER_ERROR_PREFIX: &str = "Failed to render preview frame: ";

fn invalidate_preview_output(editor_context: &mut EditorContext) {
    editor_context.preview_texture = None;
    editor_context.preview_texture_id = None;
    editor_context.preview_texture_width = 0;
    editor_context.preview_texture_height = 0;
    editor_context.preview_region = None;
}

fn clear_preview_render_error(editor_context: &mut EditorContext) {
    if editor_context
        .interaction
        .active_modal_error
        .as_deref()
        .is_some_and(|message| message.starts_with(PREVIEW_RENDER_ERROR_PREFIX))
    {
        editor_context.interaction.active_modal_error = None;
    }
}

fn report_preview_render_error(error: &library::LibraryError, editor_context: &mut EditorContext) {
    let message = format!("{PREVIEW_RENDER_ERROR_PREFIX}{error}");
    if editor_context.interaction.active_modal_error.as_deref() != Some(&message) {
        log::error!("{message}");
        editor_context.interaction.active_modal_error = Some(message);
    }
    invalidate_preview_output(editor_context);
    editor_context.preview_nontransparent_pixels = None;
    editor_context.preview_pixel_hash = None;
}

fn dispatch_preview_frame(
    frame: Result<library::model::frame::frame::FrameInfo, library::LibraryError>,
    editor_context: &mut EditorContext,
    send: impl FnOnce(library::model::frame::frame::FrameInfo),
) -> bool {
    match frame {
        Ok(frame) => {
            if editor_context
                .interaction
                .active_modal_error
                .as_deref()
                .is_some_and(|message| message.starts_with(PREVIEW_EVALUATION_ERROR_PREFIX))
            {
                editor_context.interaction.active_modal_error = None;
            }
            send(frame);
            true
        }
        Err(error) => {
            let message = format!("{PREVIEW_EVALUATION_ERROR_PREFIX}{error}");
            if editor_context.interaction.active_modal_error.as_deref() != Some(&message) {
                log::error!("{message}");
                editor_context.interaction.active_modal_error = Some(message);
            }
            invalidate_preview_output(editor_context);
            false
        }
    }
}

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
    crate::qa::register_component_with_metadata(
        "preview.canvas",
        "preview_canvas",
        preview_rect,
        true,
        Some(serde_json::json!({
            "pan": {"x": editor_context.view.pan.x, "y": editor_context.view.pan.y},
            "zoom": editor_context.view.zoom,
            "texture_width": editor_context.preview_texture_width,
            "texture_height": editor_context.preview_texture_height,
        })),
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

    let current_composition_view = project.read().ok().and_then(|project| {
        editor_context
            .get_current_composition(&project)
            .map(|composition| (composition.id, composition.width, composition.height))
    });

    // A new/changed composition always gets a default fit. While that default
    // view remains untouched, keep it centered through dock and DPI-driven
    // logical-size changes. Once the user pans or zooms, resizing preserves
    // their chosen camera.
    if let Some((composition_id, width, height)) = current_composition_view {
        let runtime = &mut editor_context.interaction.preview_viewport;
        let composition_changed = runtime.fitted_composition_id != Some(composition_id)
            || runtime.fitted_canvas_size != [width, height];
        if composition_changed {
            runtime.fitted_composition_id = Some(composition_id);
            runtime.fitted_canvas_size = [width, height];
            runtime.auto_fit = true;
        }

        let viewport_resized =
            (runtime.last_viewport_size - preview_rect.size()).length_sq() > 0.25;
        if runtime.auto_fit && (composition_changed || viewport_resized) {
            if let Some(fitted) =
                fit_canvas_to_viewport(preview_rect, egui::vec2(width as f32, height as f32))
            {
                editor_context.view.pan = fitted.pan;
                editor_context.view.zoom = fitted.zoom;
            }
        }
        runtime.last_viewport_size = preview_rect.size();
    } else {
        let runtime = &mut editor_context.interaction.preview_viewport;
        runtime.fitted_composition_id = None;
        runtime.fitted_canvas_size = [0, 0];
        runtime.last_viewport_size = preview_rect.size();
        runtime.auto_fit = true;
    }

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

    let mut state = PreviewViewportState {
        pan: &mut editor_context.view.pan,
        zoom: &mut editor_context.view.zoom,
    };

    let mut controller = ViewportController::new(
        ui,
        ui.make_persistent_id("unique_preview_viewport_controller_id"),
        None,
    )
    .with_config(ViewportConfig {
        zoom_uniform: true,
        min_zoom: PREVIEW_MIN_ZOOM,
        max_zoom: PREVIEW_MAX_ZOOM,
        ..Default::default()
    })
    // Keep a latched pan alive after Space is released. When no button is
    // down, `pan_requested` is included only to show the hand cursor.
    .with_pan_tool_active(
        gesture_decision.pan_owned || (!gesture_input.primary_down && pan_requested),
    )
    .with_zoom_tool_active(
        editor_context.view.active_tool == PreviewTool::Zoom && !gesture_decision.pan_owned,
    );

    // Provide specific rect to controller (excluding bottom bar)
    let (viewport_changed, response) = controller.interact_with_rect(
        preview_rect,
        &mut state,
        &mut editor_context.interaction.handled_hand_tool_drag,
    );

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
    let mut frame_evaluation_failed = false;
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
                        Some(valid_region.clone()),
                        &property_evaluators,
                        &plugin_manager,
                    );

                    frame_evaluation_failed =
                        !dispatch_preview_frame(frame_info, editor_context, |frame_info| {
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

        // Always drain the RenderServer, but never apply an earlier successful
        // result after the current Project failed frame evaluation.
        if let Some(result) = latest_result.filter(|_| !frame_evaluation_failed) {
            match result.output {
                Ok(output) => {
                    clear_preview_render_error(editor_context);
                    editor_context.preview_region = result.frame_info.region.clone();
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
                }
                Err(error) => {
                    report_preview_render_error(&error, editor_context);
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
            // Project order is authoritative: Composition -> Track -> Clip.
            // Preview properties/content come from each Clip's output Node.
            let mut layers = Vec::new();
            for track_id in &comp.track_ids {
                let Some(track) = proj_read.get_track(*track_id) else {
                    continue;
                };
                for clip_id in &track.clip_ids {
                    let Some(clip) = proj_read.get_clip(*clip_id) else {
                        continue;
                    };
                    let node = clip
                        .output_node_id
                        .and_then(|node_id| proj_read.get_node(node_id))
                        .or_else(|| {
                            clip.node_ids
                                .iter()
                                .find_map(|node_id| proj_read.get_node(*node_id))
                        });
                    if let Some(node) = node {
                        layers.push((clip, node, track.id));
                    }
                }
            }

            for (timeline_clip, entity, track_id) in layers {
                let current_time = editor_context.timeline.current_time as f64;
                let local_time = timeline_clip.local_time(current_time);
                let asset_opt = match &entity.content {
                    library::model::NodeContent::Media(media) => {
                        proj_read.get_asset(media.asset_id)
                    }
                    _ => None,
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
                    entity.styles.hash(&mut hasher);
                    entity.effects.hash(&mut hasher);
                    entity.effectors.hash(&mut hasher);
                    entity.decorators.hash(&mut hasher);
                    local_time.to_bits().hash(&mut hasher);
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
                        let property_evaluators = plugin_manager.get_property_evaluators();

                        let ctx = library::plugin::entity_converter::FrameEvaluationContext {
                            project: &proj_read,
                            composition: comp,
                            property_evaluators: &property_evaluators,
                            plugin_manager: &plugin_manager,
                            resolved_inputs: None,
                        };

                        let kind_str = match &entity.content {
                            library::model::NodeContent::Media(m) => {
                                if let Some(asset) =
                                    proj_read.assets.iter().find(|a| a.id == m.asset_id)
                                {
                                    match asset.kind {
                                        AssetKind::Audio => "Audio",
                                        AssetKind::Video => "Video",
                                        AssetKind::Image => "Image",
                                        _ => "Media",
                                    }
                                } else {
                                    "Media"
                                }
                            }
                            library::model::NodeContent::Generator(g) => match g {
                                library::model::GeneratorContent::Shape => "shape",
                                library::model::GeneratorContent::Text => "text",
                                library::model::GeneratorContent::SkSL => "sksl",
                                _ => "generator",
                            },
                            library::model::NodeContent::Reference(_) => "Reference",
                            library::model::NodeContent::PluginOperation(operation) => {
                                operation.operation.as_str()
                            }
                            library::model::NodeContent::Merge => "Merge",
                        };

                        if let Some(converter) = plugin_manager.get_entity_converter(kind_str) {
                            if let Some((x, y, w, h)) =
                                converter.get_bounds(&ctx, entity, local_time)
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
                }

                // Log Gizmo Time Calculation (throttle slightly if possible, or just spam per user request)
                if editor_context.timeline.current_time.fract() < 0.1 {
                    log::info!(
                        "[Gizmo] Entity: {} | CurrentTime: {:.4} | LocalTime: {:.4}",
                        entity.id,
                        current_time,
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
                    clip: timeline_clip,
                    node: entity,
                    track_id,
                    transform,
                    content_bounds,
                };
                gui_clips.push(gc);
            }
        }

        // Interactions
        {
            let mut interactions = interaction::PreviewInteractions::new(
                ui,
                editor_context,
                &project,
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
                &project,
                &gui_clips,
                to_screen,
                !gesture_decision.pan_owned,
            );
        } else if editor_context.view.active_tool == PreviewTool::Shape {
            if let Some(state) = &editor_context.interaction.vector_editor_state {
                if let Some(id) = editor_context.selection.selected_entities.iter().next() {
                    if let Some(gc) = gui_clips.iter().find(|c| c.id() == *id) {
                        if let Some(path) = gc.node.properties.get_string("path") {
                            let path = crate::ui::panels::preview::vector_editor::svg_parser::parse_svg_path(&path);
                            let renderer = crate::ui::panels::preview::vector_editor::renderer::VectorEditorRenderer {
                                state,
                                path: &path,
                                transform: gc.transform.clone(),
                                to_screen: Box::new(|p| to_screen(p)),
                            };
                            renderer.draw(ui.painter());
                        }
                    }
                }
            }
        }
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

    // Execute pending actions
    let mut history_commit_requested = false;
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
                match crate::utils::property::update_property(
                    project_service,
                    comp_id,
                    track_id,
                    entity_id,
                    &prop_name,
                    time,
                    value,
                ) {
                    Ok(()) => {}
                    Err(error) => log::error!("Failed to update Preview property: {error}"),
                }
            }
            PreviewAction::CommitHistory => history_commit_requested = true,
        }
    }
    if history_commit_requested {
        // Drag updates are applied on preceding frames, so the release frame
        // can contain only CommitHistory. Deduplication keeps this a no-op when
        // no Project value changed.
        if let Ok(project) = project.read() {
            history_manager.push_project_state(project.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn fit_uses_offset_viewport_rect_and_centers_canvas() {
        let viewport =
            egui::Rect::from_min_size(egui::pos2(137.0, 83.0), egui::vec2(1000.0, 700.0));
        let canvas_size = egui::vec2(1920.0, 1080.0);
        let fitted = fit_canvas_to_viewport(viewport, canvas_size).unwrap();
        let screen_canvas =
            egui::Rect::from_min_size(viewport.min + fitted.pan, canvas_size * fitted.zoom);

        assert_near(screen_canvas.center().x, viewport.center().x);
        assert_near(screen_canvas.center().y, viewport.center().y);
        assert!(screen_canvas.left() >= viewport.left());
        assert!(screen_canvas.right() <= viewport.right());
        assert!(screen_canvas.top() >= viewport.top());
        assert!(screen_canvas.bottom() <= viewport.bottom());
    }

    #[test]
    fn fit_is_stable_in_logical_points_across_high_dpi_scales() {
        let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(800.0, 600.0));
        let canvas_size = egui::vec2(1920.0, 1080.0);
        let fitted = fit_canvas_to_viewport(viewport, canvas_size).unwrap();

        for pixels_per_point in [1.0_f32, 1.5, 2.0, 3.0] {
            let physical_viewport_center = viewport.center().to_vec2() * pixels_per_point;
            let logical_canvas_center =
                viewport.min.to_vec2() + fitted.pan + canvas_size * fitted.zoom * 0.5;
            let physical_canvas_center = logical_canvas_center * pixels_per_point;
            assert_near(physical_canvas_center.x, physical_viewport_center.x);
            assert_near(physical_canvas_center.y, physical_viewport_center.y);
        }
    }

    #[test]
    fn frame_error_is_reported_and_invalidates_stale_preview_without_dispatch() {
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.preview_texture_id = Some(42);
        editor_context.preview_texture_width = 1920;
        editor_context.preview_texture_height = 1080;
        editor_context.preview_region = Some(library::model::frame::frame::Region {
            x: 10.0,
            y: 20.0,
            width: 640.0,
            height: 360.0,
        });
        let dispatched = Cell::new(false);

        let submitted = dispatch_preview_frame(
            Err(library::LibraryError::InvalidCompositionIndex(7)),
            &mut editor_context,
            |_| dispatched.set(true),
        );

        assert!(!submitted);
        assert!(!dispatched.get());
        assert_eq!(editor_context.preview_texture_id, None);
        assert_eq!(editor_context.preview_texture_width, 0);
        assert_eq!(editor_context.preview_texture_height, 0);
        assert_eq!(editor_context.preview_region, None);
        let message = editor_context
            .interaction
            .active_modal_error
            .as_deref()
            .expect("LibraryError should reach the existing modal error path");
        assert!(message.starts_with("Failed to evaluate preview frame:"));
        assert!(message.contains('7'));
    }

    #[test]
    fn render_error_invalidates_stale_output_and_only_its_success_clears_the_modal() {
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.preview_texture_id = Some(42);
        editor_context.preview_texture_width = 1920;
        editor_context.preview_texture_height = 1080;
        editor_context.preview_nontransparent_pixels = Some(10);
        editor_context.preview_pixel_hash = Some(20);

        report_preview_render_error(
            &library::LibraryError::Render("injected shader failure".to_string()),
            &mut editor_context,
        );

        assert_eq!(editor_context.preview_texture_id, None);
        assert_eq!(editor_context.preview_texture_width, 0);
        assert_eq!(editor_context.preview_texture_height, 0);
        assert_eq!(editor_context.preview_nontransparent_pixels, None);
        assert_eq!(editor_context.preview_pixel_hash, None);
        let message = editor_context
            .interaction
            .active_modal_error
            .as_deref()
            .unwrap();
        assert!(message.starts_with(PREVIEW_RENDER_ERROR_PREFIX));
        assert!(message.contains("injected shader failure"));

        clear_preview_render_error(&mut editor_context);
        assert_eq!(editor_context.interaction.active_modal_error, None);

        editor_context.interaction.active_modal_error = Some("unrelated failure".to_string());
        clear_preview_render_error(&mut editor_context);
        assert_eq!(
            editor_context.interaction.active_modal_error.as_deref(),
            Some("unrelated failure")
        );
    }

    #[test]
    fn space_pan_owns_press_through_modifier_release_and_pointer_release() {
        let mut owner = PreviewPrimaryGesture::Idle;
        let pressed = arbitrate_primary_gesture(
            &mut owner,
            PreviewGestureInput {
                primary_pressed: true,
                primary_down: true,
                press_started_in_viewport: true,
                pan_requested: true,
                ..Default::default()
            },
        );
        assert_eq!(owner, PreviewPrimaryGesture::Pan);
        assert!(pressed.pan_owned);

        let modifier_released = arbitrate_primary_gesture(
            &mut owner,
            PreviewGestureInput {
                primary_down: true,
                primary_dragging: true,
                pan_requested: false,
                ..Default::default()
            },
        );
        assert_eq!(owner, PreviewPrimaryGesture::Pan);
        assert!(modifier_released.pan_owned);

        let released = arbitrate_primary_gesture(
            &mut owner,
            PreviewGestureInput {
                primary_released: true,
                ..Default::default()
            },
        );
        assert!(released.pan_owned);
        assert!(released.finish_after_frame);
    }

    #[test]
    fn space_can_claim_pending_press_but_not_started_content_drag() {
        let mut pending_owner = PreviewPrimaryGesture::Idle;
        arbitrate_primary_gesture(
            &mut pending_owner,
            PreviewGestureInput {
                primary_pressed: true,
                primary_down: true,
                press_started_in_viewport: true,
                ..Default::default()
            },
        );
        assert_eq!(pending_owner, PreviewPrimaryGesture::Pending);

        let claimed = arbitrate_primary_gesture(
            &mut pending_owner,
            PreviewGestureInput {
                primary_down: true,
                pan_requested: true,
                ..Default::default()
            },
        );
        assert!(claimed.pan_owned);

        let mut content_owner = PreviewPrimaryGesture::Pending;
        let started = arbitrate_primary_gesture(
            &mut content_owner,
            PreviewGestureInput {
                primary_down: true,
                primary_dragging: true,
                ..Default::default()
            },
        );
        assert!(!started.pan_owned);
        assert_eq!(content_owner, PreviewPrimaryGesture::Content);

        let modifier_changed = arbitrate_primary_gesture(
            &mut content_owner,
            PreviewGestureInput {
                primary_down: true,
                primary_dragging: true,
                pan_requested: true,
                ..Default::default()
            },
        );
        assert!(!modifier_changed.pan_owned);
        assert_eq!(content_owner, PreviewPrimaryGesture::Content);
    }
}
