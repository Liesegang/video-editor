//! Preview surface for the authoring Timeline model.
//!
//! The panel submits the sole authoring Project and its derived hierarchical
//! RenderPlan directly to the render worker. It never reconstructs the retired
//! graph-backed Project model or exposes ordinary Timeline structure as Nodes.

mod direct_edit;
mod gizmo;
mod gizmo_geometry;
mod path_editor;
mod text_editor;
mod view;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use library::core::render_plan::{RenderPlan, RenderPlanCache};
use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, InstancePath, ProjectRevision, TimelineId};
use library::model::frame::frame::Region;
use library::rendering::renderer::RenderOutput;
use library::{RenderRequestId, RenderResult, RenderServer};
use ordered_float::OrderedFloat;
use pan_zoom_ui::CanvasTransform;

use crate::state::authoring::AuthoringUiState;

use direct_edit::handle_direct_edit;
use view::{
    navigate, paint_empty_preview, paint_preview_background, preview_canvas_transform,
    preview_content_rect, toolbar, update_fit, visible_region,
};

const TOP_BAR_HEIGHT: f32 = 34.0;
const BOTTOM_BAR_HEIGHT: f32 = 24.0;
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1_u64 << 63);

#[derive(Clone, Debug, PartialEq)]
struct PreviewRequestKey {
    revision: ProjectRevision,
    timeline_id: TimelineId,
    instance_path: Option<InstancePath>,
    frame_number: i64,
    render_scale: OrderedFloat<f64>,
    region: Option<Region>,
    /// Hash of an ephemeral direct-edit buffer. It invalidates only Preview
    /// requests and is never persisted as Project state.
    transient_edit: Option<u64>,
}

impl PreviewRequestKey {
    /// Whether two requests differ only by their Timeline frame.
    ///
    /// A late frame may be presented during uninterrupted forward playback,
    /// but never after the Project, Timeline instance, scale, or ROI changed.
    fn has_same_presentation_as(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.timeline_id == other.timeline_id
            && self.instance_path == other.instance_path
            && self.render_scale == other.render_scale
            && self.region == other.region
            && self.transient_edit == other.transient_edit
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlaybackSequence {
    started: Instant,
    anchor_frame: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewIntent {
    key: PreviewRequestKey,
    playback: Option<PlaybackSequence>,
}

struct DesiredRender {
    intent: PreviewIntent,
    project: Arc<AuthoringProject>,
    plan: Arc<RenderPlan>,
}

struct InFlightRender {
    request_id: RenderRequestId,
    intent: PreviewIntent,
}

struct PublishableRender {
    intent: PreviewIntent,
    result: RenderResult,
}

struct TransientProjectProjection {
    revision: ProjectRevision,
    upstream_edit: Option<u64>,
    edit: u64,
    project: Arc<AuthoringProject>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TransientProjectionStage {
    Text,
    InspectorProperty,
    Transform,
}

#[derive(Default)]
struct TransientProjectionCache {
    entries: HashMap<TransientProjectionStage, TransientProjectProjection>,
}

impl TransientProjectionCache {
    fn project(
        &mut self,
        stage: TransientProjectionStage,
        revision: ProjectRevision,
        upstream_edit: Option<u64>,
        edit: Option<u64>,
        source: &Arc<AuthoringProject>,
        apply: impl FnOnce(&Arc<AuthoringProject>) -> (Arc<AuthoringProject>, Option<u64>),
    ) -> (Arc<AuthoringProject>, Option<u64>) {
        let Some(edit) = edit else {
            self.entries.remove(&stage);
            return (Arc::clone(source), None);
        };
        if let Some(cached) = self.entries.get(&stage).filter(|cached| {
            cached.revision == revision
                && cached.upstream_edit == upstream_edit
                && cached.edit == edit
        }) {
            return (Arc::clone(&cached.project), Some(edit));
        }

        let (projected, applied_edit) = apply(source);
        if applied_edit == Some(edit) {
            self.entries.insert(
                stage,
                TransientProjectProjection {
                    revision,
                    upstream_edit,
                    edit,
                    project: Arc::clone(&projected),
                },
            );
        } else {
            self.entries.remove(&stage);
        }
        (projected, applied_edit)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuthoringPreviewDiagnostics {
    pub in_flight_request: Option<u64>,
    pub desired_pending: bool,
    pub submitted: u64,
    pub published: u64,
    pub discarded: u64,
    pub coalesced: u64,
}

/// Non-persisted compiler and request state owned by one Preview panel.
///
/// Keeping this state separate from [`AuthoringUiState`] makes the panel easy
/// to embed without putting RenderPlan/cache internals into the editing model.
#[derive(Default)]
pub struct AuthoringPreviewRuntime {
    plan_cache: RenderPlanCache,
    project_revision: Option<ProjectRevision>,
    project: Option<Arc<AuthoringProject>>,
    plan: Option<Arc<RenderPlan>>,
    plan_error: Option<(ProjectRevision, String)>,
    /// Immutable direct-edit projections shared by Text, Inspector values,
    /// and the transform gizmo. An unchanged edit reuses its Arc instead of
    /// cloning a large Project again on every repaint.
    transient_projections: TransientProjectionCache,
    latest: Option<PreviewIntent>,
    desired: Option<DesiredRender>,
    in_flight: Option<InFlightRender>,
    settled: Option<PreviewRequestKey>,
    displayed: Option<PreviewIntent>,
    /// Exact evaluated geometry that produced the displayed pixels.
    displayed_frame_info: Option<library::model::frame::frame::FrameInfo>,
    reported_error: Option<String>,
    submitted: u64,
    published: u64,
    discarded: u64,
    coalesced: u64,
}

impl AuthoringPreviewRuntime {
    pub fn diagnostics(&self) -> AuthoringPreviewDiagnostics {
        AuthoringPreviewDiagnostics {
            in_flight_request: self
                .in_flight
                .as_ref()
                .map(|request| request.request_id.get()),
            desired_pending: self.desired.is_some(),
            submitted: self.submitted,
            published: self.published,
            discarded: self.discarded,
            coalesced: self.coalesced,
        }
    }

    /// Atomically snapshot the sole authoring Project and incrementally derive
    /// the matching RenderPlan. Menu actions such as current-frame export may
    /// reuse this boundary instead of compiling a second plan.
    pub fn snapshot_and_plan(
        &mut self,
        service: &TimelineEditorService,
    ) -> Result<(ProjectRevision, Arc<AuthoringProject>, Arc<RenderPlan>), String> {
        let observed_revision = service.revision().map_err(|error| error.to_string())?;
        if self.project_revision == Some(observed_revision) {
            if let Some((_, error)) = self
                .plan_error
                .as_ref()
                .filter(|(revision, _)| *revision == observed_revision)
            {
                return Err(error.clone());
            }
            if let (Some(project), Some(plan)) = (&self.project, &self.plan) {
                return Ok((observed_revision, Arc::clone(project), Arc::clone(plan)));
            }
        }

        let (project, revision) = service
            .snapshot_with_revision()
            .map_err(|error| error.to_string())?;
        match self.plan_cache.compile(project.as_ref()) {
            Ok((plan, _)) => {
                let plan = Arc::new(plan);
                self.project_revision = Some(revision);
                self.project = Some(Arc::clone(&project));
                self.plan = Some(Arc::clone(&plan));
                self.plan_error = None;
                Ok((revision, project, plan))
            }
            Err(error) => {
                self.project_revision = Some(revision);
                self.project = Some(project);
                self.plan = None;
                self.plan_error = Some((revision, error.clone()));
                Err(error)
            }
        }
    }

    fn request(
        &mut self,
        key: PreviewRequestKey,
        playback: Option<PlaybackSequence>,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
    ) {
        let intent = PreviewIntent { key, playback };
        self.latest = Some(intent.clone());
        if self
            .in_flight
            .as_ref()
            .is_some_and(|request| request.intent.key == intent.key)
            || self.settled.as_ref() == Some(&intent.key)
            || self
                .desired
                .as_ref()
                .is_some_and(|request| request.intent == intent)
        {
            return;
        }
        if self.desired.is_some() {
            self.coalesced = self.coalesced.wrapping_add(1);
        }
        self.desired = Some(DesiredRender {
            intent,
            project,
            plan,
        });
    }

    fn project_transient_edit(
        &mut self,
        stage: TransientProjectionStage,
        revision: ProjectRevision,
        upstream_edit: Option<u64>,
        edit: Option<u64>,
        project: &Arc<AuthoringProject>,
        apply: impl FnOnce(&Arc<AuthoringProject>) -> (Arc<AuthoringProject>, Option<u64>),
    ) -> (Arc<AuthoringProject>, Option<u64>) {
        self.transient_projections
            .project(stage, revision, upstream_edit, edit, project, apply)
    }

    fn suspend(&mut self) {
        self.latest = None;
        self.desired = None;
    }

    fn poll(&mut self, render_server: &RenderServer) -> Option<PublishableRender> {
        let mut publishable = None;
        while let Ok(result) = render_server.poll_authoring_result() {
            let Some(in_flight) = self
                .in_flight
                .as_ref()
                .filter(|request| request.request_id == result.request_id)
            else {
                continue;
            };
            let intent = in_flight.intent.clone();
            self.in_flight = None;
            let rendered_image = matches!(&result.output, Ok(RenderOutput::Image(_)));
            if completion_is_publishable(
                &intent,
                self.latest.as_ref(),
                self.displayed.as_ref(),
                rendered_image,
            ) {
                self.settled = Some(intent.key.clone());
                self.published = self.published.wrapping_add(1);
                publishable = Some(PublishableRender { intent, result });
            } else {
                self.discarded = self.discarded.wrapping_add(1);
            }
        }
        publishable
    }

    fn submit(&mut self, render_server: &RenderServer) -> bool {
        if self.in_flight.is_some() {
            return true;
        }
        let Some(desired) = self.desired.take() else {
            return true;
        };
        let request_id = next_request_id();
        if !render_server.send_authoring_request_at_instance(
            request_id,
            Arc::clone(&desired.project),
            Arc::clone(&desired.plan),
            desired.intent.key.timeline_id,
            desired.intent.key.instance_path.clone(),
            desired.intent.key.frame_number,
            desired.intent.key.render_scale.into_inner(),
            desired.intent.key.region,
        ) {
            self.settled = Some(desired.intent.key);
            self.discarded = self.discarded.wrapping_add(1);
            return false;
        }
        self.submitted = self.submitted.wrapping_add(1);
        self.in_flight = Some(InFlightRender {
            request_id,
            intent: desired.intent,
        });
        true
    }

    fn is_busy(&self) -> bool {
        self.in_flight.is_some() || self.desired.is_some()
    }

    fn report_error(&mut self, state: &mut AuthoringUiState, error: String) {
        self.reported_error = Some(error.clone());
        state.error = Some(error);
    }

    fn clear_reported_error(&mut self, state: &mut AuthoringUiState) {
        if state.error.as_ref() == self.reported_error.as_ref() {
            state.error = None;
        }
        self.reported_error = None;
    }
}

/// Decide whether a completed request may replace the currently displayed
/// pixels. Exact requests are always authoritative. During one uninterrupted
/// playback sequence, a successful older image is also useful as long as it
/// moves presentation forward and every non-time rendering input still
/// matches. This prevents the Preview from remaining frozen merely because
/// rendering takes longer than one Timeline frame.
fn completion_is_publishable(
    completed: &PreviewIntent,
    latest: Option<&PreviewIntent>,
    displayed: Option<&PreviewIntent>,
    rendered_image: bool,
) -> bool {
    let Some(latest) = latest else {
        return false;
    };
    if completed.key == latest.key {
        return true;
    }
    if !rendered_image
        || completed.playback.is_none()
        || completed.playback != latest.playback
        || !completed.key.has_same_presentation_as(&latest.key)
        || completed.key.frame_number > latest.key.frame_number
    {
        return false;
    }
    displayed.is_none_or(|displayed| {
        displayed.playback != completed.playback
            || !displayed.key.has_same_presentation_as(&completed.key)
            || displayed.key.frame_number <= completed.key.frame_number
    })
}

/// Draw and drive the Preview.
///
/// The function is independent from the graph-backed `EditorContext` model. A
/// caller owns one [`AuthoringPreviewRuntime`] beside its transient
/// [`AuthoringUiState`].
pub fn preview_panel(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    render_server: &RenderServer,
    runtime: &mut AuthoringPreviewRuntime,
) {
    let available = ui.available_rect_before_wrap();
    let _panel_response = ui.allocate_rect(available, egui::Sense::hover());
    let top_bar = egui::Rect::from_min_size(
        available.min,
        egui::vec2(available.width(), TOP_BAR_HEIGHT.min(available.height())),
    );
    let bottom_height = BOTTOM_BAR_HEIGHT.min((available.height() - top_bar.height()).max(0.0));
    let viewport = egui::Rect::from_min_max(
        egui::pos2(available.min.x, top_bar.max.y),
        egui::pos2(available.max.x, available.max.y - bottom_height),
    );
    let bottom_bar =
        egui::Rect::from_min_max(egui::pos2(available.min.x, viewport.max.y), available.max);

    let snapshot = runtime.snapshot_and_plan(service);
    let path_tool_enabled = snapshot
        .as_ref()
        .is_ok_and(|(_, project, _)| path_editor::selected_path_is_editable(project, state));
    let text_tool_enabled = snapshot
        .as_ref()
        .is_ok_and(|(_, project, _)| text_editor::selected_text_is_editable(project, state));
    toolbar(ui, top_bar, state, text_tool_enabled, path_tool_enabled);
    let (revision, project, plan, timeline) = match snapshot {
        Ok((revision, project, plan)) => {
            let timeline = project.timelines.get(&state.active_timeline_id).cloned();
            (revision, project, plan, timeline)
        }
        Err(error) => {
            runtime.suspend();
            runtime.report_error(state, format!("Preview plan: {error}"));
            paint_empty_preview(ui, viewport, state);
            bottom_status(ui, bottom_bar, state, runtime);
            register_qa(viewport, None, state, runtime, None);
            return;
        }
    };
    let Some(timeline) = timeline else {
        runtime.suspend();
        runtime.report_error(state, "Preview Timeline no longer exists".to_string());
        paint_empty_preview(ui, viewport, state);
        bottom_status(ui, bottom_bar, state, runtime);
        register_qa(viewport, None, state, runtime, Some(revision));
        return;
    };

    update_fit(&mut state.preview, &timeline, viewport);
    let viewport_response = navigate(ui, viewport, &mut state.preview);
    let canvas_transform = preview_canvas_transform(viewport, &state.preview);
    let canvas_size = egui::vec2(timeline.width as f32, timeline.height as f32);
    let content_rect = preview_content_rect(canvas_transform, canvas_size);
    let interaction_frame = runtime
        .displayed
        .as_ref()
        .filter(|displayed| {
            displayed.key.revision == revision
                && displayed.key.timeline_id == timeline.id
                && displayed.key.frame_number == state.timeline.current_frame
        })
        .and(runtime.displayed_frame_info.as_ref());
    handle_direct_edit(
        ui,
        &viewport_response,
        viewport,
        content_rect,
        canvas_transform,
        interaction_frame,
        revision,
        project.as_ref(),
        state,
        service,
    );
    paint_preview_background(
        ui,
        viewport,
        content_rect,
        canvas_transform,
        state.preview.show_grid,
    );

    if let Some(region) = visible_region(viewport, canvas_transform, canvas_size) {
        let render_scale =
            f64::from((state.preview.canvas.zoom.x * ui.ctx().pixels_per_point()).clamp(0.01, 1.0));
        let playback = if state.timeline.is_playing {
            state
                .timeline
                .playback_anchor
                .map(|(started, anchor_frame)| PlaybackSequence {
                    started,
                    anchor_frame,
                })
        } else {
            None
        };
        let text_digest = text_editor::transient_edit_digest(state);
        let (render_project, text_edit) = runtime.project_transient_edit(
            TransientProjectionStage::Text,
            revision,
            None,
            text_digest,
            &project,
            |source| text_editor::transient_render_project(source, state),
        );
        let inspector_digest = inspector_transient_edit_digest(revision, state);
        let (render_project, inspector_edit) = runtime.project_transient_edit(
            TransientProjectionStage::InspectorProperty,
            revision,
            text_edit,
            inspector_digest,
            &render_project,
            |source| project_inspector_transient_edit(source, revision, state),
        );
        let upstream_edit = combine_transient_edits(text_edit, inspector_edit);
        let transform_digest = gizmo::transient_edit_digest(state);
        let (render_project, transform_edit) = runtime.project_transient_edit(
            TransientProjectionStage::Transform,
            revision,
            upstream_edit,
            transform_digest,
            &render_project,
            |source| gizmo::transient_render_project(source, state),
        );
        let transient_edit = combine_transient_edits(upstream_edit, transform_edit);
        runtime.request(
            PreviewRequestKey {
                revision,
                timeline_id: timeline.id,
                instance_path: state.active_instance_path.clone(),
                frame_number: state.timeline.current_frame,
                render_scale: OrderedFloat(render_scale),
                region: Some(region),
                transient_edit,
            },
            playback,
            render_project,
            plan,
        );
    } else {
        runtime.suspend();
    }

    if let Some(completed) = runtime.poll(render_server) {
        apply_result(ui.ctx(), completed, state, runtime);
    }
    if !runtime.submit(render_server) {
        runtime.report_error(state, "Preview render worker is unavailable".to_string());
    }

    paint_texture(
        ui,
        viewport,
        content_rect,
        canvas_transform,
        timeline.id,
        state,
        runtime,
    );
    let displayed_edit_frame = runtime
        .displayed
        .as_ref()
        .filter(|displayed| {
            displayed.key.revision == revision
                && displayed.key.timeline_id == timeline.id
                && displayed.key.frame_number == state.timeline.current_frame
        })
        .and(runtime.displayed_frame_info.as_ref());
    gizmo::paint_gizmo(
        ui,
        viewport,
        canvas_transform,
        displayed_edit_frame,
        project.as_ref(),
        state,
    );
    text_editor::text_editor_overlay(
        ui,
        viewport,
        canvas_transform,
        revision,
        displayed_edit_frame,
        project.as_ref(),
        state,
        service,
    );
    path_editor::path_editor_overlay(
        ui,
        &viewport_response,
        viewport,
        canvas_transform,
        revision,
        displayed_edit_frame,
        project.as_ref(),
        state,
        service,
    );
    ui.painter().rect_stroke(
        content_rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(70)),
        egui::StrokeKind::Middle,
    );
    bottom_status(ui, bottom_bar, state, runtime);
    register_qa(
        viewport,
        Some((timeline.id, timeline.width, timeline.height, content_rect)),
        state,
        runtime,
        Some(revision),
    );

    if runtime.is_busy() || state.timeline.is_playing {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
}

fn apply_result(
    context: &egui::Context,
    completed: PublishableRender,
    state: &mut AuthoringUiState,
    runtime: &mut AuthoringPreviewRuntime,
) {
    let PublishableRender { intent, result } = completed;
    let RenderResult {
        output, frame_info, ..
    } = result;
    match output {
        Ok(RenderOutput::Image(image)) => {
            let size = [image.width as usize, image.height as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &image.data);
            if let Some(texture) = &mut state.preview.texture {
                texture.set(color_image, egui::TextureOptions::LINEAR);
            } else {
                state.preview.texture = Some(context.load_texture(
                    "preview.texture",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            state.preview.texture_width = image.width;
            state.preview.texture_height = image.height;
            state.preview.rendered_revision = Some(intent.key.revision.get());
            state.preview.rendered_frame = Some(intent.key.frame_number);
            if crate::qa::is_enabled() {
                let (nontransparent, hash) = rgba_probe(&image.data);
                state.preview.nontransparent_pixels = Some(nontransparent);
                state.preview.pixel_hash = Some(hash);
            } else {
                state.preview.nontransparent_pixels = None;
                state.preview.pixel_hash = None;
            }
            runtime.displayed = Some(intent);
            runtime.displayed_frame_info = Some(frame_info);
            runtime.clear_reported_error(state);
        }
        Ok(RenderOutput::Working(_)) => runtime.report_error(
            state,
            "Preview received unterminated working-color pixels".to_string(),
        ),
        Ok(RenderOutput::Texture(_)) => runtime.report_error(
            state,
            "Authoring Preview received an unsupported unowned GPU texture".to_string(),
        ),
        Err(error) => runtime.report_error(state, format!("Preview render: {error}")),
    }
}

fn paint_texture(
    ui: &egui::Ui,
    viewport: egui::Rect,
    content: egui::Rect,
    transform: CanvasTransform,
    timeline_id: TimelineId,
    state: &AuthoringUiState,
    runtime: &AuthoringPreviewRuntime,
) {
    let (Some(texture), Some(displayed)) = (&state.preview.texture, &runtime.displayed) else {
        return;
    };
    if displayed.key.timeline_id != timeline_id {
        return;
    }
    let draw_rect = match displayed.key.region {
        Some(region) => transform
            .world_rect_to_screen(egui::Rect::from_min_size(
                egui::pos2(region.x as f32, region.y as f32),
                egui::vec2(region.width as f32, region.height as f32),
            ))
            .unwrap_or(egui::Rect::NOTHING),
        None => content,
    };
    ui.painter().with_clip_rect(viewport).image(
        texture.id(),
        draw_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn bottom_status(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    state: &AuthoringUiState,
    runtime: &AuthoringPreviewRuntime,
) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.weak(format!("Frame {}", state.timeline.current_frame));
            ui.separator();
            if runtime.is_busy() {
                ui.spinner();
                ui.weak("Rendering");
            } else if let Some(error) = runtime.reported_error.as_deref() {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            } else {
                ui.weak(&state.status);
            }
        });
    });
}

fn register_button_qa(id: &str, action: &str, response: &egui::Response, selected: bool) {
    crate::qa::register_component_with_metadata(
        id,
        "preview_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "action": action,
            "selected": selected,
        })),
    );
}

fn register_qa(
    viewport: egui::Rect,
    content: Option<(TimelineId, u64, u64, egui::Rect)>,
    state: &AuthoringUiState,
    runtime: &AuthoringPreviewRuntime,
    revision: Option<ProjectRevision>,
) {
    let diagnostics = runtime.diagnostics();
    crate::qa::register_component_with_metadata(
        "preview.canvas",
        "preview_canvas",
        viewport,
        true,
        Some(serde_json::json!({
            "timeline_id": content.map(|value| value.0),
            "project_revision": revision.map(ProjectRevision::get),
            "frame": state.timeline.current_frame,
            "screen_origin": {"x": viewport.min.x, "y": viewport.min.y},
            "pan": {"x": state.preview.canvas.pan.x, "y": state.preview.canvas.pan.y},
            "zoom": state.preview.canvas.zoom.x,
            "auto_fit": state.preview.auto_fit,
            "show_grid": state.preview.show_grid,
            "rendered_revision": state.preview.rendered_revision,
            "rendered_frame": state.preview.rendered_frame,
            "texture_width": state.preview.texture_width,
            "texture_height": state.preview.texture_height,
            "nontransparent_pixels": state.preview.nontransparent_pixels,
            "pixel_hash": state.preview.pixel_hash,
            "render_in_flight_request": diagnostics.in_flight_request,
            "render_desired_pending": diagnostics.desired_pending,
            "render_submitted": diagnostics.submitted,
            "render_published": diagnostics.published,
            "render_discarded": diagnostics.discarded,
            "render_coalesced": diagnostics.coalesced,
        })),
    );
    if let Some((timeline_id, width, height, rect)) = content {
        crate::qa::register_component_with_metadata(
            "preview.content",
            "preview_content",
            rect.intersect(viewport),
            true,
            Some(serde_json::json!({
                "timeline_id": timeline_id,
                "canvas_width": width,
                "canvas_height": height,
                "screen_origin": {"x": viewport.min.x, "y": viewport.min.y},
                "pan": {"x": state.preview.canvas.pan.x, "y": state.preview.canvas.pan.y},
                "zoom": state.preview.canvas.zoom.x,
                "auto_fit": state.preview.auto_fit,
                "show_grid": state.preview.show_grid,
            })),
        );
    }
}

fn rgba_probe(data: &[u8]) -> (u64, u64) {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    let nontransparent = data.chunks_exact(4).filter(|pixel| pixel[3] != 0).count() as u64;
    (nontransparent, hash)
}

fn next_request_id() -> RenderRequestId {
    let value = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    RenderRequestId::new(if value == 0 { 1 } else { value })
}

fn combine_transient_edits(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    match (first, second) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => Some(value),
        (Some(first), Some(second)) => Some(first.rotate_left(17) ^ second.rotate_right(11)),
    }
}

fn inspector_transient_edit_digest(
    revision: ProjectRevision,
    state: &AuthoringUiState,
) -> Option<u64> {
    state
        .inspector
        .transient_property_edit
        .as_ref()
        .filter(|edit| edit.source_revision == revision)
        .map(crate::state::authoring::TransientPropertyEdit::digest)
}

fn project_inspector_transient_edit(
    project: &Arc<AuthoringProject>,
    revision: ProjectRevision,
    state: &AuthoringUiState,
) -> (Arc<AuthoringProject>, Option<u64>) {
    let Some(edit) = state
        .inspector
        .transient_property_edit
        .as_ref()
        .filter(|edit| edit.source_revision == revision)
    else {
        return (Arc::clone(project), None);
    };
    let digest = edit.digest();
    match TimelineEditorService::project_authored_property_values(
        project,
        edit.owner,
        vec![edit.update.clone()],
    ) {
        Ok(projected) => (Arc::new(projected), Some(digest)),
        Err(_) => (Arc::clone(project), None),
    }
}

#[cfg(test)]
mod tests;
