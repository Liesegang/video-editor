use egui::Ui;
use egui_phosphor::regular as icons;
use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};

use library::model::project::Project;
use library::EditorService;
use library::RenderServer;

use crate::command::{CommandId, CommandRegistry};
use crate::state::context_types::{
    PreviewPrimaryGesture, PreviewTool, PreviewViewportRuntimeState,
};
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};
use crate::{action::HistoryManager, state::context::EditorContext};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidGlPreviewTexture {
    id: NonZeroU32,
    width: i32,
    height: i32,
}

impl ValidGlPreviewTexture {
    fn new(id: u32, width: u32, height: u32) -> Option<Self> {
        Some(Self {
            id: NonZeroU32::new(id)?,
            width: i32::try_from(width).ok().filter(|width| *width > 0)?,
            height: i32::try_from(height).ok().filter(|height| *height > 0)?,
        })
    }
}

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

fn preview_content_rect(
    viewport_rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    canvas_size: egui::Vec2,
) -> Option<egui::Rect> {
    let screen_size = canvas_size * zoom;
    if !pan.x.is_finite()
        || !pan.y.is_finite()
        || !zoom.is_finite()
        || zoom <= 0.0
        || !screen_size.x.is_finite()
        || !screen_size.y.is_finite()
        || screen_size.x <= 0.0
        || screen_size.y <= 0.0
    {
        return None;
    }

    Some(egui::Rect::from_min_size(
        viewport_rect.min + pan,
        screen_size,
    ))
}

fn register_preview_qa_components(
    preview_rect: egui::Rect,
    composition: Option<(uuid::Uuid, u64, u64)>,
    editor_context: &EditorContext,
) {
    if !crate::qa::is_enabled() {
        return;
    }

    let preview_content = composition.and_then(|(composition_id, width, height)| {
        preview_content_rect(
            preview_rect,
            editor_context.view.pan,
            editor_context.view.zoom,
            egui::vec2(width as f32, height as f32),
        )
        .map(|rect| (composition_id, width, height, rect))
    });
    crate::qa::register_component_with_metadata(
        "preview.canvas",
        "preview_canvas",
        preview_rect,
        true,
        Some(serde_json::json!({
            "pan": {"x": editor_context.view.pan.x, "y": editor_context.view.pan.y},
            "zoom": editor_context.view.zoom,
            "auto_fit": editor_context.interaction.preview_viewport.auto_fit,
            "primary_gesture": format!(
                "{:?}",
                editor_context.interaction.preview_viewport.primary_gesture
            ),
            "composition_id": preview_content.map(|content| content.0),
            "texture_width": editor_context.preview_texture_width,
            "texture_height": editor_context.preview_texture_height,
        })),
    );
    if let Some((composition_id, width, height, content_rect)) = preview_content {
        crate::qa::register_component_with_metadata(
            "preview.content",
            "preview_composition_content",
            content_rect,
            true,
            Some(serde_json::json!({
                "composition_id": composition_id,
                "canvas_width": width,
                "canvas_height": height,
                "pan": {"x": editor_context.view.pan.x, "y": editor_context.view.pan.y},
                "zoom": editor_context.view.zoom,
                "auto_fit": editor_context.interaction.preview_viewport.auto_fit,
            })),
        );
    }
}

/// Keep the derived Preview camera fitted without putting presentation state
/// into the authoritative Project.
///
/// A composition change or an explicit [`PreviewViewportRuntimeState::request_fit`]
/// applies a new centered fit. Viewport resizes continue to refit only while
/// the user has not panned or zoomed away from that default view.
fn update_preview_fit(
    runtime: &mut PreviewViewportRuntimeState,
    pan: &mut egui::Vec2,
    zoom: &mut f32,
    composition: Option<(uuid::Uuid, u64, u64)>,
    viewport_rect: egui::Rect,
) -> bool {
    let Some((composition_id, width, height)) = composition else {
        runtime.fitted_composition_id = None;
        runtime.fitted_canvas_size = [0, 0];
        runtime.last_viewport_size = viewport_rect.size();
        runtime.auto_fit = true;
        return false;
    };

    let composition_changed = runtime.fitted_composition_id != Some(composition_id)
        || runtime.fitted_canvas_size != [width, height];
    if composition_changed {
        runtime.fitted_composition_id = Some(composition_id);
        runtime.fitted_canvas_size = [width, height];
        runtime.auto_fit = true;
    }

    let viewport_resized = (runtime.last_viewport_size - viewport_rect.size()).length_sq() > 0.25;
    let fitted = if runtime.auto_fit && (composition_changed || viewport_resized) {
        fit_canvas_to_viewport(viewport_rect, egui::vec2(width as f32, height as f32))
    } else {
        None
    };
    runtime.last_viewport_size = viewport_rect.size();

    if let Some(fitted) = fitted {
        *pan = fitted.pan;
        *zoom = fitted.zoom;
        true
    } else {
        false
    }
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

fn apply_owned_primary_pan(
    pan_owned: bool,
    primary_down: bool,
    pointer_delta: egui::Vec2,
    pan: &mut egui::Vec2,
    handled_hand_tool_drag: &mut bool,
) -> bool {
    if !pan_owned || !primary_down || pointer_delta == egui::Vec2::ZERO {
        return false;
    }

    *pan += pointer_delta;
    *handled_hand_tool_drag = true;
    true
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
    editor_context.preview_frame_info = None;
    editor_context.interaction.preview_selected_instance_path = None;
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

fn preview_result_is_current(
    frame_evaluation_failed: bool,
    requested: Option<&library::model::frame::frame::FrameInfo>,
    completed: &library::model::frame::frame::FrameInfo,
) -> bool {
    !frame_evaluation_failed && requested == Some(completed)
}

fn apply_preview_actions(
    actions: Vec<PreviewAction>,
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    let mut history_commit_requested = false;
    for action in actions {
        match action {
            PreviewAction::UpdateProperty {
                node_id,
                prop_name,
                time,
                value,
            } => {
                if let Err(error) = crate::utils::property::update_node_property(
                    project_service,
                    node_id,
                    &prop_name,
                    time,
                    value,
                ) {
                    log::error!("Failed to update Preview property: {error}");
                }
            }
            PreviewAction::CommitHistory => history_commit_requested = true,
        }
    }
    if history_commit_requested {
        // A release-only frame is valid after updates from preceding drag
        // frames. HistoryManager deduplicates a true no-op (including a frame
        // with no evaluated visual source) instead of creating history-only
        // edits.
        if let Ok(project) = project.read() {
            history_manager.push_project_state(project.clone());
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
                zoom_uniform: true,
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
        gesture_input.primary_down,
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
        if let Some(result) = latest_result.filter(|result| {
            preview_result_is_current(
                frame_evaluation_failed,
                requested_frame_info.as_ref(),
                &result.frame_info,
            )
        }) {
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

        let gui_clips = editor_context
            .preview_frame_info
            .as_ref()
            .map(|frame| clip::from_evaluated_frame(&proj_read, frame))
            .unwrap_or_default();

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
                if let Some(id) = editor_context.selection.selected_entities.iter().next() {
                    if let Some(gc) = clip::visual_for_selection(
                        &gui_clips,
                        *id,
                        editor_context
                            .interaction
                            .preview_selected_instance_path
                            .as_deref(),
                    ) {
                        if let Some(path) = gc.node.properties.get_string("path") {
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

    apply_preview_actions(pending_actions, project_service, project, history_manager);

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

    fn empty_preview_frame(time: f64) -> library::model::frame::frame::FrameInfo {
        library::model::frame::frame::FrameInfo {
            width: 1920,
            height: 1080,
            background_color: library::model::frame::color::Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: ordered_float::OrderedFloat(1.0),
            now_time: ordered_float::OrderedFloat(time),
            region: None,
            items: Vec::new(),
        }
    }

    #[test]
    fn stale_preview_worker_results_never_replace_the_current_request() {
        let current = empty_preview_frame(3.0);
        let stale = empty_preview_frame(2.0);

        assert!(preview_result_is_current(false, Some(&current), &current));
        assert!(!preview_result_is_current(false, Some(&current), &stale));
        assert!(!preview_result_is_current(false, None, &current));
        assert!(!preview_result_is_current(true, Some(&current), &current));
    }

    #[test]
    fn shared_gl_texture_validation_rejects_invalid_ffi_inputs() {
        assert_eq!(
            ValidGlPreviewTexture::new(7, 1920, 1080).map(|texture| (
                texture.id.get(),
                texture.width,
                texture.height
            )),
            Some((7, 1920, 1080))
        );
        assert!(ValidGlPreviewTexture::new(0, 1920, 1080).is_none());
        assert!(ValidGlPreviewTexture::new(7, 0, 1080).is_none());
        assert!(ValidGlPreviewTexture::new(7, 1920, 0).is_none());
        assert!(ValidGlPreviewTexture::new(7, u32::MAX, 1080).is_none());
        assert!(ValidGlPreviewTexture::new(7, 1920, u32::MAX).is_none());
    }

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
            preview_content_rect(viewport, fitted.pan, fitted.zoom, canvas_size).unwrap();

        assert_near(screen_canvas.center().x, viewport.center().x);
        assert_near(screen_canvas.center().y, viewport.center().y);
        assert!(screen_canvas.left() >= viewport.left());
        assert!(screen_canvas.right() <= viewport.right());
        assert!(screen_canvas.top() >= viewport.top());
        assert!(screen_canvas.bottom() <= viewport.bottom());
    }

    #[test]
    fn preview_content_rect_rejects_invalid_camera_geometry() {
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let canvas = egui::vec2(640.0, 360.0);

        assert!(preview_content_rect(viewport, egui::Vec2::ZERO, 1.0, canvas).is_some());
        assert!(preview_content_rect(viewport, egui::Vec2::ZERO, 0.0, canvas).is_none());
        assert!(preview_content_rect(viewport, egui::Vec2::NAN, 1.0, canvas).is_none());
        assert!(preview_content_rect(viewport, egui::Vec2::ZERO, 1.0, egui::Vec2::ZERO).is_none());
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
    fn initial_and_requested_fit_center_content_without_touching_project() {
        let composition_id = uuid::Uuid::new_v4();
        let composition = Some((composition_id, 1920, 1080));
        let viewport = egui::Rect::from_min_size(egui::pos2(91.0, 57.0), egui::vec2(900.0, 620.0));
        let canvas_size = egui::vec2(1920.0, 1080.0);
        let mut runtime = PreviewViewportRuntimeState::default();
        let mut pan = egui::vec2(-400.0, 900.0);
        let mut zoom = 7.0;

        assert!(update_preview_fit(
            &mut runtime,
            &mut pan,
            &mut zoom,
            composition,
            viewport,
        ));
        let initially_fitted = preview_content_rect(viewport, pan, zoom, canvas_size).unwrap();
        assert_near(initially_fitted.center().x, viewport.center().x);
        assert_near(initially_fitted.center().y, viewport.center().y);

        // A user-authored camera survives ordinary frames and resizes once
        // auto-fit has been disabled by an interaction.
        runtime.auto_fit = false;
        pan = egui::vec2(13.0, 29.0);
        zoom = 1.75;
        let resized = viewport.expand(80.0);
        assert!(!update_preview_fit(
            &mut runtime,
            &mut pan,
            &mut zoom,
            composition,
            resized,
        ));
        assert_eq!(pan, egui::vec2(13.0, 29.0));
        assert_eq!(zoom, 1.75);

        runtime.request_fit();
        assert!(update_preview_fit(
            &mut runtime,
            &mut pan,
            &mut zoom,
            composition,
            resized,
        ));
        let reset_fit = preview_content_rect(resized, pan, zoom, canvas_size).unwrap();
        assert_near(reset_fit.center().x, resized.center().x);
        assert_near(reset_fit.center().y, resized.center().y);
    }

    fn run_preview_interaction_frame(
        context: &egui::Context,
        editor_context: &mut EditorContext,
        _project: &Arc<RwLock<Project>>,
        pending_actions: &mut Vec<PreviewAction>,
        frame: usize,
        events: Vec<egui::Event>,
    ) -> PreviewGestureDecision {
        let mut decision = PreviewGestureDecision::default();
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let viewport = ui.available_rect_before_wrap().shrink(24.0);
                    let pan_requested = ui.input(|input| input.key_down(egui::Key::Space));
                    let input = ui.input(|input| PreviewGestureInput {
                        primary_pressed: input.pointer.button_pressed(egui::PointerButton::Primary),
                        primary_down: input.pointer.button_down(egui::PointerButton::Primary),
                        primary_released: input
                            .pointer
                            .button_released(egui::PointerButton::Primary),
                        primary_dragging: input.pointer.is_decidedly_dragging(),
                        press_started_in_viewport: input
                            .pointer
                            .press_origin()
                            .is_some_and(|position| viewport.contains(position)),
                        pan_requested,
                    });
                    decision = arbitrate_primary_gesture(
                        &mut editor_context.interaction.preview_viewport.primary_gesture,
                        input,
                    );

                    let pointer_delta = ui.input(|input| input.pointer.delta());
                    let (mut viewport_changed, response) = {
                        let mut viewport_state = PreviewViewportState {
                            pan: &mut editor_context.view.pan,
                            zoom: &mut editor_context.view.zoom,
                        };
                        let controller_id = ui.make_persistent_id("preview-space-pan-real-events");
                        let mut controller = ViewportController::new(ui, controller_id, None);
                        controller.interact_with_rect(
                            viewport,
                            &mut viewport_state,
                            &mut editor_context.interaction.handled_hand_tool_drag,
                        )
                    };
                    viewport_changed |= apply_owned_primary_pan(
                        decision.pan_owned,
                        input.primary_down,
                        pointer_delta,
                        &mut editor_context.view.pan,
                        &mut editor_context.interaction.handled_hand_tool_drag,
                    );
                    if viewport_changed {
                        editor_context.interaction.preview_viewport.auto_fit = false;
                    }

                    let mut interactions = interaction::PreviewInteractions::new(
                        ui,
                        editor_context,
                        &[],
                        |position| position,
                        |position| position,
                    );
                    interactions.handle(&response, viewport, decision.pan_owned, pending_actions);
                    drop(interactions);

                    if decision.finish_after_frame {
                        editor_context.interaction.preview_viewport.primary_gesture =
                            PreviewPrimaryGesture::Idle;
                    }
                });
            },
        );
        decision
    }

    fn run_transformed_visual_frame(
        context: &egui::Context,
        editor_context: &mut EditorContext,
        visual: &clip::PreviewClip,
        pending_actions: &mut Vec<PreviewAction>,
        frame: usize,
        events: Vec<egui::Event>,
    ) {
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let viewport = ui.available_rect_before_wrap();
                    let response = ui.interact(
                        viewport,
                        ui.make_persistent_id("preview-transformed-visual"),
                        egui::Sense::click_and_drag(),
                    );
                    let visuals = std::slice::from_ref(visual);
                    let mut interactions = interaction::PreviewInteractions::new(
                        ui,
                        editor_context,
                        visuals,
                        |position| position,
                        |position| position,
                    );
                    interactions.handle(&response, viewport, false, pending_actions);
                    drop(interactions);
                    gizmo::draw_gizmo(ui, editor_context, visuals, |position| position, true);
                });
            },
        );
    }

    fn raw_pointer_drag(
        context: &egui::Context,
        editor_context: &mut EditorContext,
        visual: &clip::PreviewClip,
        pending_actions: &mut Vec<PreviewAction>,
        start: egui::Pos2,
        threshold: egui::Pos2,
        end: egui::Pos2,
    ) {
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            0,
            Vec::new(),
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            1,
            vec![egui::Event::PointerMoved(start)],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            2,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            3,
            vec![egui::Event::PointerMoved(threshold)],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            4,
            vec![egui::Event::PointerMoved(end)],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            5,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }

    #[test]
    fn real_space_primary_drag_pans_without_content_selection_or_drag_updates() {
        use crate::model::ui_types::GizmoHandle;
        use crate::model::vector::VectorEditorState;
        use crate::state::context_types::{BodyDragState, GizmoState};
        use library::model::vector::HandleType;
        use std::collections::HashMap;

        let context = egui::Context::default();
        let composition_id = uuid::Uuid::new_v4();
        let selected_id = uuid::Uuid::new_v4();
        let project = Arc::new(RwLock::new(Project::new("preview gesture")));
        let project_before = project.read().unwrap().clone();
        let mut editor_context = EditorContext::new(composition_id);
        editor_context
            .selection
            .selected_entities
            .insert(selected_id);
        editor_context.selection.last_selected_entity_id = Some(selected_id);
        let mut pending_actions = Vec::new();

        // Warm egui's widget memory before the real key/pointer sequence.
        assert!(
            !run_preview_interaction_frame(
                &context,
                &mut editor_context,
                &project,
                &mut pending_actions,
                0,
                Vec::new(),
            )
            .pan_owned
        );

        editor_context.interaction.is_moving_selected_entity = true;
        editor_context.interaction.preview_selection_drag_start = Some(egui::pos2(4.0, 5.0));
        editor_context.interaction.body_drag_state = Some(BodyDragState {
            start_mouse_pos: egui::pos2(4.0, 5.0),
            original_positions: HashMap::from([(selected_id, [12.0, 34.0])]),
        });
        editor_context.interaction.gizmo_state = Some(GizmoState {
            start_mouse_pos: egui::pos2(4.0, 5.0),
            active_handle: GizmoHandle::Rotation,
            original_position: [12.0, 34.0],
            original_scale_x: 100.0,
            original_scale_y: 100.0,
            original_rotation: 0.0,
            original_visual_position: [12.0, 34.0],
            original_visual_scale_x: 100.0,
            original_visual_scale_y: 100.0,
            original_visual_rotation: 0.0,
            original_anchor_x: 0.0,
            original_anchor_y: 0.0,
            original_width: 100.0,
            original_height: 100.0,
        });
        editor_context.interaction.vector_editor_state = Some(VectorEditorState {
            selected_handle: Some((0, HandleType::Vertex)),
            ..Default::default()
        });

        let pan_before = editor_context.view.pan;
        let zoom_before = editor_context.view.zoom;
        let start = egui::pos2(180.0, 160.0);
        let midpoint = egui::pos2(255.0, 202.5);
        let end = egui::pos2(330.0, 245.0);
        let key_event = |pressed| egui::Event::Key {
            key: egui::Key::Space,
            physical_key: Some(egui::Key::Space),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        let pressed = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            1,
            vec![
                key_event(true),
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(pressed.pan_owned);

        let dragged = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            2,
            vec![egui::Event::PointerMoved(midpoint)],
        );
        assert!(dragged.pan_owned);
        assert_near(
            editor_context.view.pan.x,
            pan_before.x + midpoint.x - start.x,
        );
        assert_near(
            editor_context.view.pan.y,
            pan_before.y + midpoint.y - start.y,
        );

        let modifier_released = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            3,
            vec![key_event(false)],
        );
        assert!(modifier_released.pan_owned);
        assert!(!modifier_released.finish_after_frame);

        let dragged_after_modifier_release = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            4,
            vec![egui::Event::PointerMoved(end)],
        );
        assert!(dragged_after_modifier_release.pan_owned);
        assert_near(editor_context.view.pan.x, pan_before.x + end.x - start.x);
        assert_near(editor_context.view.pan.y, pan_before.y + end.y - start.y);
        assert_eq!(editor_context.view.zoom, zoom_before);
        assert!(!editor_context.interaction.preview_viewport.auto_fit);

        let released = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            5,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(released.pan_owned);
        assert!(released.finish_after_frame);
        assert_eq!(
            editor_context.interaction.preview_viewport.primary_gesture,
            PreviewPrimaryGesture::Idle
        );

        assert_eq!(
            editor_context.selection.selected_entities,
            [selected_id].into_iter().collect()
        );
        assert_eq!(
            editor_context.selection.last_selected_entity_id,
            Some(selected_id)
        );
        assert!(!editor_context.interaction.is_moving_selected_entity);
        assert!(editor_context.interaction.body_drag_state.is_none());
        assert!(editor_context
            .interaction
            .preview_selection_drag_start
            .is_none());
        assert!(editor_context.interaction.gizmo_state.is_none());
        assert!(editor_context
            .interaction
            .vector_editor_state
            .as_ref()
            .is_some_and(|state| state.selected_handle.is_none()));
        assert!(pending_actions.is_empty());
        assert_eq!(*project.read().unwrap(), project_before);
        assert_eq!(editor_context.view.zoom, zoom_before);
        assert!(!editor_context.interaction.preview_viewport.auto_fit);
    }

    #[test]
    fn raw_input_parent_transform_drags_edit_source_space_without_baking_downstream_transform() {
        use library::cache::CacheManager;
        use library::model::frame::transform::{Position, Scale, Transform};
        use library::model::property::{Property, PropertyValue, Vec2 as PropertyVec2};
        use library::model::{GeneratorContent, Node, NodeContent};
        use library::plugin::PluginManager;
        use library::rendering::renderer::Affine2D;
        use ordered_float::OrderedFloat;

        fn source_node() -> Node {
            let mut source = Node::new(
                "Transformed source",
                NodeContent::Generator(GeneratorContent::SkSL),
            );
            source.properties.set(
                "position".to_string(),
                Property::constant(PropertyValue::Vec2(PropertyVec2 {
                    x: OrderedFloat(7.0),
                    y: OrderedFloat(11.0),
                })),
            );
            source.properties.set(
                "scale".to_string(),
                Property::constant(PropertyValue::Vec2(PropertyVec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                })),
            );
            source.properties.set(
                "rotation".to_string(),
                Property::constant(PropertyValue::Number(OrderedFloat(0.0))),
            );
            source
        }

        fn transformed_visual(source: &Node) -> clip::PreviewClip {
            let source_transform = Transform {
                position: Position { x: 7.0, y: 11.0 },
                ..Transform::default()
            };
            // This is the final value after a downstream Transform Effector.
            // It deliberately differs from the directly editable source.
            let transform = Transform {
                position: Position { x: 20.0, y: 30.0 },
                scale: Scale { x: 2.0, y: 1.0 },
                ..Transform::default()
            };
            let parent_transform = Affine2D::from(&Transform {
                position: Position { x: 300.0, y: 150.0 },
                scale: Scale { x: 2.0, y: 0.5 },
                rotation: 90.0,
                ..Transform::default()
            });
            clip::PreviewClip {
                node: source.clone(),
                track_id: None,
                source_transform,
                world_transform: parent_transform.compose(Affine2D::from(&transform)),
                parent_transform,
                transform,
                content_bounds: Some((-20.0, -20.0, 40.0, 40.0)),
                instance_path: vec![source.id],
            }
        }

        fn apply_actions(source: Node, actions: Vec<PreviewAction>) -> Node {
            let source_id = source.id;
            let mut model = Project::new("transformed preview edit");
            model.add_node(source);
            let project = Arc::new(RwLock::new(model));
            let service = EditorService::new(
                Arc::clone(&project),
                Arc::new(PluginManager::default()),
                Arc::new(CacheManager::new()),
            )
            .unwrap();
            let mut history = HistoryManager::new();
            history.push_project_state(project.read().unwrap().clone());
            apply_preview_actions(actions, &service, &project, &mut history);
            let edited = project.read().unwrap().get_node(source_id).unwrap().clone();
            edited
        }

        fn vector_property(node: &Node, key: &str) -> (f64, f64) {
            let Some(PropertyValue::Vec2(value)) =
                node.properties.get(key).and_then(Property::value)
            else {
                panic!("{key} must remain a Vec2 property")
            };
            (value.x.into_inner(), value.y.into_inner())
        }

        // Body drag: the final screen delta (3, 8) maps through the inverse
        // parent matrix to source-local (4, -6). It must be added to the
        // source position (7, 11), not the downstream position (20, 30).
        let source = source_node();
        let visual = transformed_visual(&source);
        let context = egui::Context::default();
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.view.zoom = 1.0;
        editor_context.select_entity(source.id, None);
        editor_context.interaction.preview_selected_instance_path =
            Some(visual.instance_path.clone());
        let (center_x, center_y) = visual.world_transform.map_point(0.0, 0.0);
        let center = egui::pos2(center_x as f32, center_y as f32);
        let mut actions = Vec::new();
        raw_pointer_drag(
            &context,
            &mut editor_context,
            &visual,
            &mut actions,
            center,
            center + egui::vec2(0.0, 8.0),
            center + egui::vec2(3.0, 16.0),
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            PreviewAction::UpdateProperty { node_id, prop_name, .. }
                if *node_id == source.id && prop_name == "position"
        )));
        assert!(actions
            .iter()
            .any(|action| matches!(action, PreviewAction::CommitHistory)));
        let body_positions = actions
            .iter()
            .filter_map(|action| match action {
                PreviewAction::UpdateProperty {
                    prop_name,
                    value: PropertyValue::Vec2(value),
                    ..
                } if prop_name == "position" => Some((value.x.into_inner(), value.y.into_inner())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(body_positions.last(), Some(&(11.0, 5.0)));
        let edited = apply_actions(source, actions);
        assert_eq!(vector_property(&edited, "position"), (11.0, 5.0));
        assert_eq!(vector_property(&edited, "scale"), (100.0, 100.0));

        // Right-handle drag captures at pointer press, so its complete screen
        // delta (0, 28) maps to +14 on the local X axis. The displayed width
        // is 80 after the downstream 2x scale, so the source scale becomes
        // 117.5% and source position moves by +7. Neither downstream 200% nor
        // its (20, 30) position is baked.
        let source = source_node();
        let visual = transformed_visual(&source);
        let context = egui::Context::default();
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.view.zoom = 1.0;
        editor_context.select_entity(source.id, None);
        editor_context.interaction.preview_selected_instance_path =
            Some(visual.instance_path.clone());
        let (right_x, right_y) = visual.world_transform.map_point(20.0, 0.0);
        let right_handle = egui::pos2(right_x as f32, right_y as f32);
        let mut actions = Vec::new();
        raw_pointer_drag(
            &context,
            &mut editor_context,
            &visual,
            &mut actions,
            right_handle,
            right_handle + egui::vec2(0.0, 8.0),
            right_handle + egui::vec2(0.0, 28.0),
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            PreviewAction::UpdateProperty { node_id, prop_name, .. }
                if *node_id == source.id && prop_name == "scale"
        )));
        assert!(actions
            .iter()
            .any(|action| matches!(action, PreviewAction::CommitHistory)));
        let edited = apply_actions(source, actions);
        assert_eq!(vector_property(&edited, "position"), (14.0, 11.0));
        assert_eq!(vector_property(&edited, "scale"), (117.5, 100.0));
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

    #[test]
    fn preview_actions_edit_the_evaluated_source_not_the_output_sink_or_history_alone() {
        use library::cache::CacheManager;
        use library::model::property::{Property, PropertyValue, Vec2};
        use library::model::{GeneratorContent, Node, NodeContent};
        use library::plugin::PluginManager;
        use ordered_float::OrderedFloat;

        let mut source = Node::new(
            "Text source",
            NodeContent::Generator(GeneratorContent::Text),
        );
        let source_id = source.id;
        source.properties.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(10.0),
                y: OrderedFloat(20.0),
            })),
        );
        let plugins = Arc::new(PluginManager::default());
        let sink = plugins.create_style_operation_node("fill").unwrap();
        let sink_id = sink.id;
        let mut model = Project::new("preview target");
        model.add_node(source);
        model.add_node(sink);
        let project = Arc::new(RwLock::new(model));
        let service =
            EditorService::new(Arc::clone(&project), plugins, Arc::new(CacheManager::new()))
                .unwrap();
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().unwrap().clone());

        apply_preview_actions(
            vec![PreviewAction::CommitHistory],
            &service,
            &project,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 1, "a no-output frame is not an edit");

        apply_preview_actions(
            vec![
                PreviewAction::UpdateProperty {
                    node_id: source_id,
                    prop_name: "position".to_string(),
                    time: 0.0,
                    value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(30.0),
                        y: OrderedFloat(40.0),
                    }),
                },
                PreviewAction::CommitHistory,
            ],
            &service,
            &project,
            &mut history,
        );

        let model = project.read().unwrap();
        assert_eq!(
            model
                .get_node(source_id)
                .unwrap()
                .properties
                .get("position")
                .and_then(Property::value),
            Some(&PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(30.0),
                y: OrderedFloat(40.0),
            }))
        );
        assert!(
            model
                .get_node(sink_id)
                .unwrap()
                .properties
                .get("position")
                .is_none(),
            "the output sink must not receive a guessed transform property"
        );
        assert_eq!(history.undo_depth(), 2);
    }
}
