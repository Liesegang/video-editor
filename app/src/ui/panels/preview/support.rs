use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};

use library::model::project::Project;
use library::EditorService;

use crate::state::context_types::{PreviewPrimaryGesture, PreviewViewportRuntimeState};
use crate::ui::viewport::ViewportState;
use crate::{action::HistoryManager, state::context::EditorContext};

use super::{action::PreviewAction, clip::PreviewClip, routing};

pub(super) const PREVIEW_FIT_PADDING: f32 = 24.0;
pub(super) const PREVIEW_MIN_ZOOM: f32 = 0.0001;
pub(super) const PREVIEW_MAX_ZOOM: f32 = 1000.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ValidGlPreviewTexture {
    pub(super) id: NonZeroU32,
    pub(super) width: i32,
    pub(super) height: i32,
}

impl ValidGlPreviewTexture {
    pub(super) fn new(id: u32, width: u32, height: u32) -> Option<Self> {
        Some(Self {
            id: NonZeroU32::new(id)?,
            width: i32::try_from(width).ok().filter(|width| *width > 0)?,
            height: i32::try_from(height).ok().filter(|height| *height > 0)?,
        })
    }
}

pub(super) fn rgba_image_probe(data: &[u8]) -> (u64, u64) {
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
pub(super) struct FittedPreviewView {
    /// Translation relative to the Preview rect's minimum corner.
    pub(super) pan: egui::Vec2,
    pub(super) zoom: f32,
}

/// Fit a composition canvas into the actual Preview allocation.
///
/// egui rectangles and pointer coordinates are expressed in logical points,
/// so pixels-per-point intentionally does not participate in this geometry.
/// It is applied later only when choosing the renderer's pixel resolution.
pub(super) fn fit_canvas_to_viewport(
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

pub(super) fn preview_content_rect(
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

/// Keep the derived Preview camera fitted without putting presentation state
/// into the authoritative Project.
///
/// A composition change or an explicit [`PreviewViewportRuntimeState::request_fit`]
/// applies a new centered fit. Viewport resizes continue to refit only while
/// the user has not panned or zoomed away from that default view.
pub(super) fn update_preview_fit(
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
pub(super) struct PreviewGestureInput {
    pub(super) primary_pressed: bool,
    pub(super) primary_down: bool,
    pub(super) primary_released: bool,
    pub(super) primary_dragging: bool,
    pub(super) press_started_in_viewport: bool,
    pub(super) pan_requested: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PreviewGestureDecision {
    pub(super) pan_owned: bool,
    pub(super) finish_after_frame: bool,
}

/// Arbitrate the primary pointer once per press, then retain that owner until
/// release. Modifier changes cannot leak one physical gesture into two tools.
pub(super) fn arbitrate_primary_gesture(
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

pub(super) fn apply_owned_primary_pan(
    pan_owned: bool,
    primary_pressed: bool,
    primary_down: bool,
    primary_released: bool,
    pointer_delta: egui::Vec2,
    pan: &mut egui::Vec2,
    handled_hand_tool_drag: &mut bool,
) -> bool {
    if !pan_owned
        || primary_pressed
        || (!primary_down && !primary_released)
        || pointer_delta == egui::Vec2::ZERO
    {
        return false;
    }

    *pan += pointer_delta;
    *handled_hand_tool_drag = true;
    true
}

/// Submit only a fully evaluated frame. Evaluation failures invalidate the
/// displayed output because keeping a previous texture would present stale
/// pixels as if they were the current Project state.
pub(super) const PREVIEW_EVALUATION_ERROR_PREFIX: &str = "Failed to evaluate preview frame: ";
pub(super) const PREVIEW_RENDER_ERROR_PREFIX: &str = "Failed to render preview frame: ";

pub(super) fn invalidate_preview_output(editor_context: &mut EditorContext) {
    editor_context.preview_texture = None;
    editor_context.preview_texture_id = None;
    editor_context.preview_texture_width = 0;
    editor_context.preview_texture_height = 0;
    editor_context.preview_region = None;
    editor_context.preview_frame_info = None;
    editor_context.interaction.preview_edit_target = None;
}

pub(super) fn clear_preview_render_error(editor_context: &mut EditorContext) {
    if editor_context
        .interaction
        .active_modal_error
        .as_deref()
        .is_some_and(|message| message.starts_with(PREVIEW_RENDER_ERROR_PREFIX))
    {
        editor_context.interaction.active_modal_error = None;
    }
}

pub(super) fn report_preview_render_error(
    error: &library::LibraryError,
    editor_context: &mut EditorContext,
) {
    let message = format!("{PREVIEW_RENDER_ERROR_PREFIX}{error}");
    if editor_context.interaction.active_modal_error.as_deref() != Some(&message) {
        log::error!("{message}");
        editor_context.interaction.active_modal_error = Some(message);
    }
    invalidate_preview_output(editor_context);
    editor_context.preview_nontransparent_pixels = None;
    editor_context.preview_pixel_hash = None;
}

pub(super) fn dispatch_preview_frame(
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

pub(super) fn preview_result_is_current(
    frame_evaluation_failed: bool,
    requested: Option<&library::model::frame::frame::FrameInfo>,
    completed: &library::model::frame::frame::FrameInfo,
) -> bool {
    !frame_evaluation_failed && requested == Some(completed)
}

/// Geometry interaction follows the synchronous current request. The
/// displayed frame is accepted only as pixel provenance and is deliberately
/// never a fallback for hit testing against a newer Project.
pub(super) fn preview_frame_for_interaction<'a>(
    requested: Option<&'a library::model::frame::frame::FrameInfo>,
    _displayed: Option<&library::model::frame::frame::FrameInfo>,
) -> Option<&'a library::model::frame::frame::FrameInfo> {
    requested
}

pub(super) fn preview_render_wait_requires_repaint(
    frame_evaluation_failed: bool,
    requested: bool,
    completed_current_request: bool,
) -> bool {
    !frame_evaluation_failed && requested && !completed_current_request
}

pub(super) fn apply_preview_actions(
    actions: Vec<PreviewAction>,
    current_visuals: &[PreviewClip],
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) -> bool {
    let had_updates = actions
        .iter()
        .any(|action| matches!(action, PreviewAction::UpdateProperty { .. }));
    let validated_actions = if let Ok(current_project) = project.read() {
        actions
            .into_iter()
            .map(|action| {
                let valid = match &action {
                    PreviewAction::UpdateProperty {
                        edit_target,
                        node_id,
                        prop_name,
                        ..
                    } => routing::can_update_property(
                        &current_project,
                        current_visuals,
                        edit_target,
                        *node_id,
                        prop_name,
                    ),
                    PreviewAction::CommitHistory => true,
                };
                (action, valid)
            })
            .collect::<Vec<_>>()
    } else {
        actions
            .into_iter()
            .map(|action| {
                let valid = matches!(action, PreviewAction::CommitHistory);
                (action, valid)
            })
            .collect()
    };

    let mut history_commit_requested = false;
    let mut changed = false;
    for (action, valid) in validated_actions {
        match action {
            PreviewAction::UpdateProperty {
                edit_target,
                node_id,
                prop_name,
                time,
                value,
            } => {
                if !valid {
                    log::warn!(
                        "Ignored stale Preview update for Node {node_id} property {prop_name} (owner {:?}, path {:?})",
                        edit_target.owner,
                        edit_target.instance_path
                    );
                    continue;
                }
                if let Err(error) = crate::utils::property::update_node_property(
                    project_service,
                    node_id,
                    &prop_name,
                    time,
                    value,
                ) {
                    log::error!("Failed to update Preview property: {error}");
                } else {
                    changed = true;
                }
            }
            PreviewAction::CommitHistory => history_commit_requested = true,
        }
    }
    if history_commit_requested && (!had_updates || changed) {
        // A release-only frame is valid after updates from preceding drag
        // frames. HistoryManager deduplicates a true no-op (including a frame
        // with no evaluated visual source) instead of creating history-only
        // edits.
        if let Ok(project) = project.read() {
            history_manager.push_project_state(project.clone());
        }
    }
    changed
}

pub(super) struct PreviewViewportState<'a> {
    pub(super) pan: &'a mut egui::Vec2,
    pub(super) zoom: &'a mut f32,
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
