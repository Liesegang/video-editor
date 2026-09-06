use super::*;

use library::model::authoring::{TimelineInterval, TimelineTrackId};
use library::model::frame::color::Color;
use library::model::property::PropertyValue;
use ordered_float::OrderedFloat;
use pan_zoom_ui::CanvasState;

mod caret_metrics;
mod node_clip;

#[test]
fn transient_digest_includes_the_target_identity() {
    let mut state = AuthoringUiState::new(library::model::authoring::TimelineId::new());
    let first = TimelineItemId::new();
    let second = TimelineItemId::new();
    state.preview.text_editor.editing = true;
    let revision = ProjectRevision::initial();
    state.preview.text_editor.target_revision = Some(revision);
    state.preview.text_editor.target_item = Some(first);
    state.preview.text_editor.buffer = "same".to_string();
    let first_digest = transient_edit_digest(&state, revision).expect("first digest");
    state.preview.text_editor.target_item = Some(second);
    let second_digest = transient_edit_digest(&state, revision).expect("second digest");

    assert_ne!(first_digest, second_digest);
}

const SCREEN: egui::Rect =
    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(800.0, 600.0));
const VIEWPORT: egui::Rect =
    egui::Rect::from_min_max(egui::Pos2::new(20.0, 30.0), egui::Pos2::new(780.0, 570.0));
const LAYOUT_BOUNDS: egui::Rect =
    egui::Rect::from_min_max(egui::Pos2::new(100.0, 80.0), egui::Pos2::new(260.0, 140.0));
const CLICK: egui::Pos2 = egui::Pos2::new(600.0, 400.0);

struct TextEditorFixture {
    service: TimelineEditorService,
    state: AuthoringUiState,
    first: TimelineItemId,
    second: TimelineItemId,
    frame: usize,
}

impl TextEditorFixture {
    fn new() -> Self {
        let service = TimelineEditorService::create_default("Preview Text editor")
            .expect("authoring service");
        let project = service.snapshot().expect("default Project");
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        let first = add_text(&service, track_id, "A", 0);
        let second = add_text(&service, track_id, "Second", 1);
        let mut state = AuthoringUiState::new(timeline_id);
        state.preview.active_tool = PreviewTool::Text;
        state.selection.replace(AuthoringSelection::Item(first));
        let revision = service.revision().expect("Project revision");
        state.preview.text_editor.begin(first, revision, "A");
        state.preview.text_editor.layout_bounds = Some(LAYOUT_BOUNDS);
        Self {
            service,
            state,
            first,
            second,
            frame: 0,
        }
    }

    fn current_project(&self) -> (Arc<AuthoringProject>, ProjectRevision) {
        self.service
            .snapshot_with_revision()
            .expect("current Project")
    }

    fn run(&mut self, context: &egui::Context, canvas: CanvasTransform, events: Vec<egui::Event>) {
        let (project, revision) = self.current_project();
        run_overlay(
            context,
            &project,
            revision,
            &mut self.state,
            &self.service,
            canvas,
            self.frame,
            events,
        );
        self.frame += 1;
    }

    fn run_click(
        &mut self,
        context: &egui::Context,
        canvas: CanvasTransform,
        evaluated: Option<&FrameInfo>,
        events: Vec<egui::Event>,
    ) -> bool {
        let (project, revision) = self.current_project();
        let handled = run_tool_click(
            context,
            &project,
            revision,
            &mut self.state,
            &self.service,
            canvas,
            evaluated,
            self.frame,
            events,
        );
        self.frame += 1;
        handled
    }
}

fn add_text(
    service: &TimelineEditorService,
    track_id: TimelineTrackId,
    text: &str,
    layer: i64,
) -> TimelineItemId {
    service
        .add_item(
            track_id,
            text.to_string(),
            SourceRef::Text {
                text: text.to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::from_whole_seconds(5)).unwrap(),
            layer,
        )
        .expect("Text item")
        .0
}

#[allow(
    clippy::too_many_arguments,
    reason = "The test drives the real overlay with the same complete frame boundary as Preview"
)]
fn run_overlay(
    context: &egui::Context,
    project: &AuthoringProject,
    revision: ProjectRevision,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    canvas: CanvasTransform,
    frame: usize,
    events: Vec<egui::Event>,
) {
    drop(context.run(raw_input(frame, events), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            text_editor_overlay(
                ui, VIEWPORT, canvas, revision, None, project, state, service,
            );
        });
    }));
}

fn canvas(pan: egui::Vec2, zoom: f32) -> CanvasTransform {
    CanvasTransform::new(VIEWPORT.min, CanvasState::uniform(pan, zoom))
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn pointer_button(pressed: bool) -> egui::Event {
    pointer_button_at(CLICK, pressed)
}

fn pointer_button_at(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn raw_input(frame: usize, events: Vec<egui::Event>) -> egui::RawInput {
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::Key { modifiers, .. } | egui::Event::PointerButton { modifiers, .. } => {
                Some(*modifiers)
            }
            _ => None,
        })
        .unwrap_or(egui::Modifiers::NONE);
    egui::RawInput {
        screen_rect: Some(SCREEN),
        time: Some(frame as f64 / 60.0),
        modifiers,
        events,
        ..egui::RawInput::default()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "The test drives the production Text-tool click boundary with its complete Preview context"
)]
fn run_tool_click(
    context: &egui::Context,
    project: &AuthoringProject,
    revision: ProjectRevision,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    canvas: CanvasTransform,
    evaluated: Option<&FrameInfo>,
    frame: usize,
    events: Vec<egui::Event>,
) -> bool {
    let plugins = PluginManager::default();
    let mut handled = false;
    drop(context.run(raw_input(frame, events), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let response = ui.interact(
                VIEWPORT,
                ui.id().with("preview-text-tool-test"),
                egui::Sense::click(),
            );
            handled = handle_tool_click(
                ui, &response, VIEWPORT, canvas, evaluated, revision, project, state, service,
                &plugins,
            );
        });
    }));
    handled
}

fn empty_frame() -> FrameInfo {
    FrameInfo {
        width: 1920,
        height: 1080,
        background_color: Color::black(),
        color_profile: "sRGB".to_string(),
        render_scale: OrderedFloat(1.0),
        now_time: OrderedFloat(0.0),
        region: None,
        items: Vec::new(),
    }
}

fn capture_pending_click(context: &egui::Context, fixture: &mut TextEditorFixture) {
    fixture.state.preview.text_editor.finish();
    fixture.state.preview.active_tool = PreviewTool::Text;
    let initial_canvas = canvas(egui::Vec2::ZERO, 1.0);
    // egui hit testing uses the previous pass's widget geometry, as in the
    // already displayed native panel before the user presses the pointer.
    fixture.run_click(
        context,
        initial_canvas,
        None,
        vec![egui::Event::PointerMoved(CLICK)],
    );
    assert!(!fixture.run_click(
        context,
        initial_canvas,
        None,
        vec![egui::Event::PointerMoved(CLICK), pointer_button(true)],
    ));
    assert!(fixture.run_click(
        context,
        initial_canvas,
        None,
        vec![egui::Event::PointerMoved(CLICK), pointer_button(false)],
    ));
    let pending = fixture
        .state
        .preview
        .text_editor
        .pending_click
        .as_ref()
        .expect("click waits for evaluated geometry");
    assert_eq!(
        pending.position,
        initial_canvas.screen_to_world(CLICK).unwrap()
    );
}

fn text(project: &AuthoringProject, item_id: TimelineItemId) -> &str {
    let SourceRef::Text { text, .. } = &project.items[&item_id].source else {
        panic!("fixture item must remain direct Text")
    };
    text
}

#[test]
fn empty_buffer_keeps_the_real_editor_alive_across_pan_and_zoom() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    let identity = canvas(egui::Vec2::ZERO, 1.0);
    fixture.run(&context, identity, Vec::new());

    fixture.run(
        &context,
        canvas(egui::vec2(35.0, -10.0), 1.75),
        vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
    );
    assert_eq!(fixture.state.preview.text_editor.buffer, "");
    assert!(fixture.state.preview.text_editor.editing);
    assert_eq!(
        fixture.state.preview.text_editor.layout_bounds,
        Some(LAYOUT_BOUNDS),
        "navigation must not rewrite the session's Composition-space bounds"
    );

    let (source, revision) = fixture.current_project();
    let (empty_projection, digest) =
        transient_render_project(&source, revision, &fixture.state).expect("empty projection");
    assert_eq!(text(&empty_projection, fixture.first), "");
    assert!(digest.is_some());
    assert_eq!(text(&source, fixture.first), "A");

    fixture.run(
        &context,
        canvas(egui::vec2(-18.0, 24.0), 0.65),
        vec![egui::Event::Text("B".to_string())],
    );
    assert_eq!(fixture.state.preview.text_editor.buffer, "B");
    assert!(fixture.state.preview.text_editor.editing);
    assert_eq!(fixture.service.revision().unwrap(), revision);

    let identity_rect = editor_rect(LAYOUT_BOUNDS, identity, VIEWPORT).expect("identity rect");
    let navigated = canvas(egui::vec2(-18.0, 24.0), 0.65);
    let navigated_rect = editor_rect(LAYOUT_BOUNDS, navigated, VIEWPORT).expect("navigated rect");
    assert_ne!(identity_rect, navigated_rect);
    assert_eq!(
        navigated_rect,
        egui::Rect::from_min_max(
            navigated.world_to_screen(LAYOUT_BOUNDS.min),
            navigated.world_to_screen(LAYOUT_BOUNDS.max),
        )
        .intersect(VIEWPORT)
    );
}

#[test]
fn stale_revision_cancels_without_overwriting_the_external_project_edit() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    fixture.state.preview.text_editor.buffer = "stale draft".to_string();
    let editing_revision = fixture.state.preview.text_editor.target_revision.unwrap();
    fixture
        .service
        .set_text(fixture.first, "external edit".to_string())
        .expect("external Text edit");
    let (current, current_revision) = fixture.current_project();
    assert_ne!(editing_revision, current_revision);

    run_overlay(
        &context,
        &current,
        current_revision,
        &mut fixture.state,
        &fixture.service,
        canvas(egui::Vec2::ZERO, 1.0),
        0,
        Vec::new(),
    );

    assert!(!fixture.state.preview.text_editor.editing);
    assert_eq!(fixture.state.preview.active_tool, PreviewTool::Select);
    assert!(fixture
        .state
        .error
        .as_deref()
        .is_some_and(|error| error.contains("cancelled")));
    assert_eq!(fixture.service.revision().unwrap(), current_revision);
    let after = fixture
        .service
        .snapshot()
        .expect("Project after cancellation");
    assert_eq!(text(&after, fixture.first), "external edit");
    assert_eq!(text(&after, fixture.second), "Second");

    let source = after;
    let (projected, digest) = transient_render_project(&source, current_revision, &fixture.state)
        .expect("cancelled editor has no projection");
    assert!(Arc::ptr_eq(&source, &projected));
    assert_eq!(digest, None);
}

#[test]
fn changing_selection_accepts_once_and_preserves_the_new_selection() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    fixture.state.preview.text_editor.buffer = "Accepted".to_string();
    fixture
        .state
        .selection
        .replace(AuthoringSelection::Item(fixture.second));
    let before = fixture.service.revision().unwrap();

    fixture.run(&context, canvas(egui::Vec2::ZERO, 1.0), Vec::new());

    assert!(!fixture.state.preview.text_editor.editing);
    assert_eq!(fixture.service.revision().unwrap().get(), before.get() + 1);
    assert_eq!(
        fixture.state.selection.primary(),
        Some(AuthoringSelection::Item(fixture.second))
    );
    let committed = fixture.service.snapshot().expect("committed Project");
    assert_eq!(text(&committed, fixture.first), "Accepted");
    assert_eq!(text(&committed, fixture.second), "Second");

    fixture
        .service
        .undo()
        .expect("Undo")
        .expect("one Text edit");
    let undone = fixture.service.snapshot().expect("undone Project");
    assert_eq!(text(&undone, fixture.first), "A");
    assert_eq!(text(&undone, fixture.second), "Second");
}

#[test]
fn escape_cancels_even_when_the_session_has_no_layout_bounds() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    fixture.state.preview.text_editor.buffer = "discarded".to_string();
    fixture.state.preview.text_editor.layout_bounds = None;
    let before = fixture
        .service
        .snapshot_with_revision()
        .expect("before Escape");

    fixture.run(
        &context,
        canvas(egui::Vec2::ZERO, 1.0),
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );

    assert!(!fixture.state.preview.text_editor.editing);
    assert_eq!(fixture.state.preview.active_tool, PreviewTool::Select);
    assert_eq!(fixture.state.status, "Cancelled Text edit");
    assert_eq!(
        fixture
            .service
            .snapshot_with_revision()
            .expect("after Escape"),
        before
    );
}

#[test]
fn command_enter_accepts_even_when_the_editor_bounds_are_offscreen() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    fixture.state.preview.text_editor.buffer = "Committed".to_string();
    fixture.state.preview.text_editor.layout_bounds = Some(egui::Rect::from_min_max(
        egui::pos2(2_000.0, 2_000.0),
        egui::pos2(2_200.0, 2_100.0),
    ));
    let before = fixture.service.revision().unwrap();

    fixture.run(
        &context,
        canvas(egui::Vec2::ZERO, 1.0),
        vec![key(
            egui::Key::Enter,
            egui::Modifiers {
                ctrl: true,
                ..egui::Modifiers::NONE
            },
        )],
    );

    assert!(!fixture.state.preview.text_editor.editing);
    assert_eq!(fixture.state.preview.active_tool, PreviewTool::Select);
    assert_eq!(fixture.service.revision().unwrap().get(), before.get() + 1);
    assert_eq!(
        text(&fixture.service.snapshot().unwrap(), fixture.first),
        "Committed"
    );
}

#[test]
fn tool_switch_with_a_stale_snapshot_cannot_overwrite_the_current_service() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    let (stale_project, stale_revision) = fixture.current_project();
    fixture.state.preview.text_editor.buffer = "stale draft".to_string();
    fixture.state.preview.active_tool = PreviewTool::Pan;
    fixture
        .service
        .set_text(fixture.first, "external edit".to_string())
        .expect("external edit");
    let current_revision = fixture.service.revision().unwrap();
    assert_ne!(current_revision, stale_revision);

    run_overlay(
        &context,
        &stale_project,
        stale_revision,
        &mut fixture.state,
        &fixture.service,
        canvas(egui::Vec2::ZERO, 1.0),
        0,
        Vec::new(),
    );

    assert!(!fixture.state.preview.text_editor.editing);
    assert_eq!(fixture.state.preview.active_tool, PreviewTool::Pan);
    assert!(fixture
        .state
        .error
        .as_deref()
        .is_some_and(|error| error.contains("cancelled")));
    assert_eq!(fixture.service.revision().unwrap(), current_revision);
    let current = fixture.service.snapshot().expect("current Project");
    assert_eq!(text(&current, fixture.first), "external edit");
    assert_eq!(text(&current, fixture.second), "Second");
}

#[test]
fn click_waits_for_geometry_then_uses_the_original_world_point_exactly_once() {
    let context = egui::Context::default();
    let mut fixture = TextEditorFixture::new();
    let before = fixture
        .service
        .snapshot_with_revision()
        .expect("before click");
    capture_pending_click(&context, &mut fixture);
    assert_eq!(
        fixture
            .service
            .snapshot_with_revision()
            .expect("pending click"),
        before,
        "press/release without evaluated geometry must not mutate the Project"
    );

    let changed_camera = canvas(egui::vec2(-240.0, 130.0), 2.5);
    assert!(fixture.run_click(&context, changed_camera, Some(&empty_frame()), Vec::new(),));
    assert!(fixture.state.preview.text_editor.pending_click.is_none());
    let created_id = fixture
        .state
        .preview
        .text_editor
        .target_item
        .expect("created Text enters editing");
    let after = fixture.service.snapshot().expect("created Project");
    assert_eq!(after.items.len(), before.0.items.len() + 1);
    let PropertyValue::Vec2(position) = after.items[&created_id]
        .authored_properties
        .get("position")
        .and_then(|property| property.value())
        .expect("created position")
    else {
        panic!("created position must be Vec2")
    };
    let expected = canvas(egui::Vec2::ZERO, 1.0)
        .screen_to_world(CLICK)
        .unwrap();
    assert_eq!(position.x.into_inner(), f64::from(expected.x));
    assert_eq!(position.y.into_inner(), f64::from(expected.y));

    let count = after.items.len();
    fixture.run_click(&context, changed_camera, Some(&empty_frame()), Vec::new());
    assert_eq!(fixture.service.snapshot().unwrap().items.len(), count);
}

#[test]
fn pending_click_is_cancelled_when_its_preview_context_changes() {
    enum Change {
        Revision,
        Tool,
        Selection,
        Frame,
        InstancePath,
        Escape,
        PrimaryOutsideViewport,
    }

    for change in [
        Change::Revision,
        Change::Tool,
        Change::Selection,
        Change::Frame,
        Change::InstancePath,
        Change::Escape,
        Change::PrimaryOutsideViewport,
    ] {
        let context = egui::Context::default();
        let mut fixture = TextEditorFixture::new();
        capture_pending_click(&context, &mut fixture);
        let before_count = fixture.service.snapshot().unwrap().items.len();
        let mut events = Vec::new();
        match change {
            Change::Revision => {
                fixture
                    .service
                    .set_text(fixture.first, "external".to_string())
                    .expect("revision change");
            }
            Change::Tool => fixture.state.preview.active_tool = PreviewTool::Pan,
            Change::Selection => fixture
                .state
                .selection
                .replace(AuthoringSelection::Item(fixture.second)),
            Change::Frame => fixture.state.timeline.current_frame += 1,
            Change::InstancePath => fixture.state.active_instance_path = None,
            Change::Escape => events.push(key(egui::Key::Escape, egui::Modifiers::NONE)),
            Change::PrimaryOutsideViewport => {
                let outside = egui::pos2(5.0, 5.0);
                events.extend([
                    egui::Event::PointerMoved(outside),
                    pointer_button_at(outside, true),
                ]);
            }
        }

        assert!(!fixture.run_click(
            &context,
            canvas(egui::vec2(50.0, 40.0), 1.2),
            Some(&empty_frame()),
            events,
        ));
        assert!(fixture.state.preview.text_editor.pending_click.is_none());
        assert_eq!(
            fixture.service.snapshot().unwrap().items.len(),
            before_count
        );
    }
}
