//! Timeline-first Preview surface.
//!
//! The panel submits the sole authoring Project and its derived hierarchical
//! RenderPlan directly to the render worker. It never builds a legacy Project
//! and never exposes ordinary Timeline structure as Nodes.

mod direct_edit;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use egui_phosphor::regular as icons;
use library::core::render_plan::{RenderPlan, RenderPlanCache};
use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, InstancePath, ProjectRevision, TimelineId};
use library::model::frame::frame::Region;
use library::rendering::renderer::RenderOutput;
use library::{RenderRequestId, RenderResult, RenderServer};
use ordered_float::OrderedFloat;
use pan_zoom_ui::{AxisMask, CanvasState, NavigationConfig};

use crate::state::authoring::{AuthoringPreviewView, AuthoringUiState, PreviewTool};
use crate::ui::viewport::{ViewportController, ViewportInputPolicy, ViewportState, ZoomPolicy};

use direct_edit::{handle_position_gesture, paint_position_gizmo};

const TOP_BAR_HEIGHT: f32 = 34.0;
const BOTTOM_BAR_HEIGHT: f32 = 24.0;
const FIT_PADDING: f32 = 24.0;
const MIN_ZOOM: f32 = 0.0001;
const MAX_ZOOM: f32 = 1000.0;
const CHECKER_SIZE: f32 = 12.0;

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1_u64 << 63);

#[derive(Clone, Debug, PartialEq)]
struct PreviewRequestKey {
    revision: ProjectRevision,
    timeline_id: TimelineId,
    instance_path: Option<InstancePath>,
    frame_number: i64,
    render_scale: OrderedFloat<f64>,
    region: Option<Region>,
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
    latest: Option<PreviewIntent>,
    desired: Option<DesiredRender>,
    in_flight: Option<InFlightRender>,
    settled: Option<PreviewRequestKey>,
    displayed: Option<PreviewIntent>,
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

/// Draw and drive the Timeline-first Preview.
///
/// The function is deliberately independent from the legacy EditorContext and
/// Project. A caller owns one [`AuthoringPreviewRuntime`] beside its transient
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

    toolbar(ui, top_bar, state);

    let snapshot = runtime.snapshot_and_plan(service);
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

    update_fit(
        &mut state.preview,
        timeline.id,
        timeline.width,
        timeline.height,
        viewport,
    );
    let viewport_response = navigate(ui, viewport, &mut state.preview);
    let content_rect = preview_content_rect(
        viewport,
        state.preview.pan,
        state.preview.zoom,
        egui::vec2(timeline.width as f32, timeline.height as f32),
    );
    handle_position_gesture(
        ui,
        &viewport_response,
        content_rect,
        revision,
        project.as_ref(),
        state,
        service,
    );
    paint_preview_background(ui, viewport, content_rect, state.preview.show_grid);

    if let Some(region) = visible_region(viewport, content_rect, state.preview.zoom) {
        let render_scale =
            f64::from((state.preview.zoom * ui.ctx().pixels_per_point()).clamp(0.01, 1.0));
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
        runtime.request(
            PreviewRequestKey {
                revision,
                timeline_id: timeline.id,
                instance_path: state.active_instance_path.clone(),
                frame_number: state.timeline.current_frame,
                render_scale: OrderedFloat(render_scale),
                region: Some(region),
            },
            playback,
            Arc::clone(&project),
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

    paint_texture(ui, viewport, content_rect, timeline.id, state, runtime);
    paint_position_gizmo(ui, viewport, content_rect, project.as_ref(), state);
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

fn toolbar(ui: &mut egui::Ui, rect: egui::Rect, state: &mut AuthoringUiState) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 0.0);
            tool_button(ui, state, PreviewTool::Select, icons::CURSOR, "select");
            tool_button(ui, state, PreviewTool::Pan, icons::HAND, "pan");
            tool_button(
                ui,
                state,
                PreviewTool::Zoom,
                icons::MAGNIFYING_GLASS,
                "zoom",
            );
            ui.separator();
            let fit = ui.button("Fit").on_hover_text("Fit Timeline to Preview");
            register_button_qa("timeline_first.preview.fit", "fit", &fit, false);
            if fit.clicked() {
                state.preview.auto_fit = true;
                state.preview.fitted_timeline = None;
            }
            let grid = ui
                .add(egui::Button::new("Grid").selected(state.preview.show_grid))
                .on_hover_text("Toggle canvas grid");
            register_button_qa(
                "timeline_first.preview.grid",
                "grid",
                &grid,
                state.preview.show_grid,
            );
            if grid.clicked() {
                state.preview.show_grid = !state.preview.show_grid;
            }
            ui.separator();
            ui.weak(format!("{:.0}%", state.preview.zoom * 100.0));
        });
    });
}

fn tool_button(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    tool: PreviewTool,
    icon: &str,
    name: &str,
) {
    let selected = state.preview.active_tool == tool;
    let response = ui
        .add(egui::Button::new(egui::RichText::new(icon).size(18.0)).selected(selected))
        .on_hover_text(format!("{} Tool", title_case(name)));
    register_button_qa(
        &format!("timeline_first.preview.tool.{name}"),
        name,
        &response,
        selected,
    );
    if response.clicked() {
        state.preview.active_tool = tool;
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn navigate(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    view: &mut AuthoringPreviewView,
) -> egui::Response {
    let mut handled_pan = false;
    let mut state = PreviewViewportState {
        pan: &mut view.pan,
        zoom: &mut view.zoom,
    };
    let config = NavigationConfig {
        input_policy: ViewportInputPolicy::Trackpad,
        zoom_policy: ZoomPolicy::Uniform,
        pan_axes: AxisMask::BOTH,
        zoom_axes: AxisMask::BOTH,
        min_zoom: egui::Vec2::splat(MIN_ZOOM),
        max_zoom: egui::Vec2::splat(MAX_ZOOM),
        ..NavigationConfig::default()
    };
    let (changed, response) = ViewportController::new(
        ui,
        ui.make_persistent_id("timeline_first_preview_viewport"),
        Some(egui::Key::Space),
    )
    .with_config(config)
    .with_pan_tool_active(view.active_tool == PreviewTool::Pan)
    .with_zoom_tool_active(view.active_tool == PreviewTool::Zoom)
    .interact_with_rect(viewport, &mut state, &mut handled_pan);
    if changed {
        view.auto_fit = false;
    }
    response
}

struct PreviewViewportState<'a> {
    pan: &'a mut egui::Vec2,
    zoom: &'a mut f32,
}

impl ViewportState for PreviewViewportState<'_> {
    fn canvas_state(&self) -> CanvasState {
        CanvasState::uniform(*self.pan, *self.zoom)
    }

    fn set_canvas_state(&mut self, state: CanvasState) {
        *self.pan = state.pan;
        *self.zoom = state.zoom.x;
    }
}

fn update_fit(
    view: &mut AuthoringPreviewView,
    timeline_id: TimelineId,
    width: u64,
    height: u64,
    viewport: egui::Rect,
) {
    let timeline_changed = view.fitted_timeline != Some(timeline_id);
    if timeline_changed {
        view.fitted_timeline = Some(timeline_id);
        view.auto_fit = true;
    }
    let resized = (view.last_viewport_size - viewport.size()).length_sq() > 0.25;
    view.last_viewport_size = viewport.size();
    if !view.auto_fit || (!timeline_changed && !resized) {
        return;
    }
    if let Some((pan, zoom)) = fit_canvas(viewport, egui::vec2(width as f32, height as f32)) {
        view.pan = pan;
        view.zoom = zoom;
    }
}

fn fit_canvas(viewport: egui::Rect, canvas: egui::Vec2) -> Option<(egui::Vec2, f32)> {
    let size = viewport.size();
    if !size.x.is_finite()
        || !size.y.is_finite()
        || !canvas.x.is_finite()
        || !canvas.y.is_finite()
        || size.x <= 0.0
        || size.y <= 0.0
        || canvas.x <= 0.0
        || canvas.y <= 0.0
    {
        return None;
    }
    let padding = egui::vec2(FIT_PADDING.min(size.x * 0.1), FIT_PADDING.min(size.y * 0.1));
    let available = egui::vec2(
        (size.x - padding.x * 2.0).max(f32::EPSILON),
        (size.y - padding.y * 2.0).max(f32::EPSILON),
    );
    let zoom = (available.x / canvas.x)
        .min(available.y / canvas.y)
        .clamp(MIN_ZOOM, MAX_ZOOM);
    let pan = (size - canvas * zoom) * 0.5;
    Some((pan, zoom))
}

fn preview_content_rect(
    viewport: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    canvas: egui::Vec2,
) -> egui::Rect {
    egui::Rect::from_min_size(viewport.min + pan, canvas * zoom)
}

fn visible_region(viewport: egui::Rect, content: egui::Rect, zoom: f32) -> Option<Region> {
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }
    let visible = viewport.intersect(content);
    if !visible.is_positive() {
        return None;
    }
    let min = (visible.min - content.min) / zoom;
    let max = (visible.max - content.min) / zoom;
    Some(Region {
        x: f64::from(min.x),
        y: f64::from(min.y),
        width: f64::from(max.x - min.x),
        height: f64::from(max.y - min.y),
    })
}

fn paint_empty_preview(ui: &egui::Ui, viewport: egui::Rect, state: &AuthoringUiState) {
    paint_preview_background(ui, viewport, viewport, state.preview.show_grid);
}

fn paint_preview_background(
    ui: &egui::Ui,
    viewport: egui::Rect,
    content: egui::Rect,
    show_grid: bool,
) {
    let painter = ui.painter().with_clip_rect(viewport);
    if show_grid {
        pan_zoom_ui::paint_canvas(
            &painter,
            viewport,
            viewport.min,
            CanvasState::uniform(egui::Vec2::ZERO, 1.0),
            pan_zoom_ui::GridConfig::default(),
            pan_zoom_ui::CanvasTheme::default(),
        );
    } else {
        painter.rect_filled(
            viewport,
            0.0,
            pan_zoom_ui::CanvasTheme::default().background,
        );
    }
    paint_checkerboard(&painter, viewport.intersect(content), content.min);
}

fn paint_checkerboard(painter: &egui::Painter, clipped: egui::Rect, origin: egui::Pos2) {
    if !clipped.is_positive() {
        return;
    }
    let first_column = ((clipped.min.x - origin.x) / CHECKER_SIZE).floor() as i64;
    let first_row = ((clipped.min.y - origin.y) / CHECKER_SIZE).floor() as i64;
    let columns = (clipped.width() / CHECKER_SIZE).ceil() as usize + 2;
    let rows = (clipped.height() / CHECKER_SIZE).ceil() as usize + 2;
    for row_offset in 0..rows {
        let row = first_row.saturating_add(row_offset as i64);
        let y = origin.y + row as f32 * CHECKER_SIZE;
        for column_offset in 0..columns {
            let column = first_column.saturating_add(column_offset as i64);
            let x = origin.x + column as f32 * CHECKER_SIZE;
            let color = if (row + column).rem_euclid(2) == 0 {
                egui::Color32::from_gray(50)
            } else {
                egui::Color32::from_gray(67)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, y), egui::Vec2::splat(CHECKER_SIZE))
                    .intersect(clipped),
                0.0,
                color,
            );
        }
    }
}

fn apply_result(
    context: &egui::Context,
    completed: PublishableRender,
    state: &mut AuthoringUiState,
    runtime: &mut AuthoringPreviewRuntime,
) {
    let PublishableRender { intent, result } = completed;
    match result.output {
        Ok(RenderOutput::Image(image)) => {
            let size = [image.width as usize, image.height as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &image.data);
            if let Some(texture) = &mut state.preview.texture {
                texture.set(color_image, egui::TextureOptions::LINEAR);
            } else {
                state.preview.texture = Some(context.load_texture(
                    "timeline_first_preview_texture",
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
        Some(region) => egui::Rect::from_min_size(
            content.min + egui::vec2(region.x as f32, region.y as f32) * state.preview.zoom,
            egui::vec2(region.width as f32, region.height as f32) * state.preview.zoom,
        ),
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
        "timeline_first_preview_control",
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
        "timeline_first.preview.canvas",
        "timeline_first_preview_canvas",
        viewport,
        true,
        Some(serde_json::json!({
            "timeline_id": content.map(|value| value.0),
            "project_revision": revision.map(ProjectRevision::get),
            "frame": state.timeline.current_frame,
            "pan": {"x": state.preview.pan.x, "y": state.preview.pan.y},
            "zoom": state.preview.zoom,
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
            "timeline_first.preview.content",
            "timeline_first_preview_content",
            rect.intersect(viewport),
            true,
            Some(serde_json::json!({
                "timeline_id": timeline_id,
                "canvas_width": width,
                "canvas_height": height,
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

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{MediaTime, RationalRate};

    fn request_key(timeline_id: TimelineId, frame_number: i64) -> PreviewRequestKey {
        PreviewRequestKey {
            revision: ProjectRevision::initial(),
            timeline_id,
            instance_path: None,
            frame_number,
            render_scale: OrderedFloat(0.5),
            region: Some(Region {
                x: 0.0,
                y: 0.0,
                width: 640.0,
                height: 360.0,
            }),
        }
    }

    fn intent(
        timeline_id: TimelineId,
        frame_number: i64,
        playback: Option<PlaybackSequence>,
    ) -> PreviewIntent {
        PreviewIntent {
            key: request_key(timeline_id, frame_number),
            playback,
        }
    }

    #[test]
    fn fit_is_centered_and_preserves_aspect_ratio() {
        let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 40.0), egui::vec2(1000.0, 600.0));
        let (pan, zoom) = fit_canvas(viewport, egui::vec2(1920.0, 1080.0)).unwrap();
        let fitted =
            egui::Rect::from_min_size(viewport.min + pan, egui::vec2(1920.0, 1080.0) * zoom);

        assert!((fitted.center() - viewport.center()).length() <= 0.001);
        assert!(fitted.width() <= viewport.width());
        assert!(fitted.height() <= viewport.height());
        assert!((fitted.width() / fitted.height() - 16.0 / 9.0).abs() <= 0.001);
    }

    #[test]
    fn visible_region_maps_screen_crop_back_to_timeline_pixels() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 80.0));
        let content = egui::Rect::from_min_size(egui::pos2(-20.0, -10.0), egui::vec2(200.0, 100.0));

        let region = visible_region(viewport, content, 0.5).unwrap();

        assert_eq!(region.x, 40.0);
        assert_eq!(region.y, 20.0);
        assert_eq!(region.width, 200.0);
        assert_eq!(region.height, 160.0);
    }

    #[test]
    fn uninterrupted_playback_publishes_a_lagged_frame_that_moves_forward() {
        let timeline_id = TimelineId::new();
        let playback = PlaybackSequence {
            started: Instant::now(),
            anchor_frame: 0,
        };
        let completed = intent(timeline_id, 18, Some(playback));
        let latest = intent(timeline_id, 55, Some(playback));
        let displayed = intent(timeline_id, 4, Some(playback));

        assert!(completion_is_publishable(
            &completed,
            Some(&latest),
            Some(&displayed),
            true,
        ));
    }

    #[test]
    fn lagged_playback_result_cannot_cross_seek_or_presentation_boundaries() {
        let timeline_id = TimelineId::new();
        let playback = PlaybackSequence {
            started: Instant::now(),
            anchor_frame: 0,
        };
        let after_seek = PlaybackSequence {
            started: playback.started + std::time::Duration::from_millis(1),
            anchor_frame: 40,
        };
        let completed = intent(timeline_id, 18, Some(playback));
        let seeked_latest = intent(timeline_id, 55, Some(after_seek));
        assert!(!completion_is_publishable(
            &completed,
            Some(&seeked_latest),
            None,
            true,
        ));

        let mut changed_roi = intent(timeline_id, 55, Some(playback));
        changed_roi.key.region.as_mut().expect("ROI").x = 32.0;
        assert!(!completion_is_publishable(
            &completed,
            Some(&changed_roi),
            None,
            true,
        ));

        let paused_latest = intent(timeline_id, 55, None);
        assert!(!completion_is_publishable(
            &completed,
            Some(&paused_latest),
            None,
            true,
        ));
    }

    #[test]
    fn lagged_playback_result_never_regresses_the_displayed_frame() {
        let timeline_id = TimelineId::new();
        let playback = PlaybackSequence {
            started: Instant::now(),
            anchor_frame: 0,
        };
        let completed = intent(timeline_id, 18, Some(playback));
        let latest = intent(timeline_id, 55, Some(playback));
        let displayed = intent(timeline_id, 24, Some(playback));

        assert!(!completion_is_publishable(
            &completed,
            Some(&latest),
            Some(&displayed),
            true,
        ));
    }

    #[test]
    fn playback_requests_keep_one_in_flight_and_one_latest_desired_frame() {
        let project = Arc::new(
            AuthoringProject::new(
                "Preview coalescing",
                1280,
                720,
                RationalRate::new(30, 1).expect("FPS"),
                MediaTime::new(10, 1).expect("duration"),
            )
            .expect("Project"),
        );
        let timeline_id = project.root_timeline_id;
        let mut cache = RenderPlanCache::default();
        let (plan, _) = cache.compile(project.as_ref()).expect("RenderPlan");
        let plan = Arc::new(plan);
        let playback = PlaybackSequence {
            started: Instant::now(),
            anchor_frame: 0,
        };
        let mut runtime = AuthoringPreviewRuntime::default();

        runtime.request(
            request_key(timeline_id, 1),
            Some(playback),
            Arc::clone(&project),
            Arc::clone(&plan),
        );
        let first = runtime.desired.take().expect("first desired request");
        runtime.in_flight = Some(InFlightRender {
            request_id: RenderRequestId::new(1),
            intent: first.intent,
        });

        for frame_number in 2..=55 {
            runtime.request(
                request_key(timeline_id, frame_number),
                Some(playback),
                Arc::clone(&project),
                Arc::clone(&plan),
            );
        }

        assert_eq!(
            runtime
                .in_flight
                .as_ref()
                .expect("single in-flight request")
                .intent
                .key
                .frame_number,
            1,
        );
        assert_eq!(
            runtime
                .desired
                .as_ref()
                .expect("latest desired request")
                .intent
                .key
                .frame_number,
            55,
        );
        assert_eq!(runtime.diagnostics().coalesced, 53);
    }
}
