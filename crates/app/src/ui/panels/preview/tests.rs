use super::*;
use crate::state::authoring::AuthoringPreviewView;
use library::model::authoring::{MediaTime, RationalRate};
use pan_zoom_ui::CanvasState;

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
        transient_edit: None,
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
    let transform =
        view::preview_fit_transform(viewport, egui::vec2(1920.0, 1080.0)).expect("fit transform");
    let fitted = preview_content_rect(transform, egui::vec2(1920.0, 1080.0));

    assert!((fitted.center() - viewport.center()).length() <= 0.001);
    assert!(fitted.width() <= viewport.width());
    assert!(fitted.height() <= viewport.height());
    assert!((fitted.width() / fitted.height() - 16.0 / 9.0).abs() <= 0.001);
}

#[test]
fn preview_content_and_grid_share_pan_zoom_transform() {
    let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(500.0, 400.0));
    let view = AuthoringPreviewView {
        canvas: CanvasState::uniform(egui::vec2(35.0, 25.0), 2.0),
        ..AuthoringPreviewView::default()
    };
    let transform = preview_canvas_transform(viewport, &view);
    let content = preview_content_rect(transform, egui::vec2(160.0, 90.0));

    assert_eq!(content.min, viewport.min + view.canvas.pan);
    assert_eq!(content.size(), egui::vec2(320.0, 180.0));

    let lines = pan_zoom_ui::grid_lines(
        viewport,
        transform,
        pan_zoom_ui::GridConfig {
            adaptive: false,
            ..pan_zoom_ui::GridConfig::default()
        },
    );
    let x_origin = lines
        .iter()
        .find(|line| {
            line.axis == pan_zoom_ui::GridAxis::X && line.kind == pan_zoom_ui::GridLineKind::Origin
        })
        .expect("visible X origin");
    let x_minor = lines
        .iter()
        .find(|line| line.axis == pan_zoom_ui::GridAxis::X && line.world_position == 20.0)
        .expect("visible X minor line");

    assert_eq!(x_origin.screen_position, content.min.x);
    assert_eq!(x_minor.screen_position, content.min.x + 40.0);
}

#[test]
fn painted_grid_uses_preview_camera_pan() {
    let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(500.0, 400.0));
    let canvas_state = CanvasState::uniform(egui::vec2(35.0, 25.0), 2.0);
    let transform = CanvasTransform::new(viewport.min, canvas_state);
    let content = preview_content_rect(transform, egui::vec2(160.0, 90.0));
    let context = egui::Context::default();
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 500.0),
            )),
            ..egui::RawInput::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint_preview_background(ui, viewport, content, transform, true);
            });
        },
    );
    let origin_stroke = pan_zoom_ui::CanvasTheme::default().origin_grid;

    let origin_segments = output.shapes.iter().filter_map(|clipped| {
        let egui::Shape::LineSegment { points, stroke } = &clipped.shape else {
            return None;
        };
        (stroke.color == origin_stroke.color && stroke.width == origin_stroke.width)
            .then_some(points)
    });

    assert!(origin_segments.into_iter().any(|points| {
        points[0].x == content.min.x
            && points[1].x == content.min.x
            && points[0].y == viewport.min.y
            && points[1].y == viewport.max.y
    }));
}

#[test]
fn visible_region_maps_screen_crop_back_to_timeline_pixels() {
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 80.0));
    let transform = CanvasTransform::new(
        viewport.min,
        CanvasState::uniform(egui::vec2(-20.0, -10.0), 0.5),
    );

    let region = visible_region(viewport, transform, egui::vec2(400.0, 200.0)).unwrap();

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

#[test]
fn transient_projection_cache_is_scoped_by_upstream_edit() {
    use std::cell::Cell;

    let source = Arc::new(
        AuthoringProject::new(
            "Transient projection cache",
            640,
            360,
            RationalRate::new(30, 1).expect("FPS"),
            MediaTime::new(2, 1).expect("duration"),
        )
        .expect("Project"),
    );
    let revision = ProjectRevision::initial();
    let applications = Cell::new(0);
    let mut cache = TransientProjectionCache::default();
    let apply = |project: &Arc<AuthoringProject>| {
        applications.set(applications.get() + 1);
        (Arc::new(project.as_ref().clone()), Some(7))
    };

    let (first, _) = cache.project(
        TransientProjectionStage::InspectorProperty,
        revision,
        Some(11),
        Some(7),
        &source,
        apply,
    );
    let (reused, _) = cache.project(
        TransientProjectionStage::InspectorProperty,
        revision,
        Some(11),
        Some(7),
        &source,
        apply,
    );
    let (reprojected, _) = cache.project(
        TransientProjectionStage::InspectorProperty,
        revision,
        Some(12),
        Some(7),
        &source,
        apply,
    );

    assert_eq!(applications.get(), 2);
    assert!(Arc::ptr_eq(&first, &reused));
    assert!(!Arc::ptr_eq(&first, &reprojected));
}

#[test]
fn inspector_drag_projects_into_preview_without_mutating_the_source_project() {
    use crate::state::authoring::TransientPropertyEdit;
    use library::editor::{
        AuthoringPropertyOwner, AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
    };
    use library::model::authoring::{SourceRef, TimelineInterval};
    use library::model::frame::color::Color;
    use library::model::property::PropertyValue;

    let service = TimelineEditorService::create_default("Inspector preview").expect("service");
    let source = service.snapshot().expect("source Project");
    let timeline_id = source.root_timeline_id;
    let track_id = source.timelines[&timeline_id].track_order[0];
    let (item_id, _) = service
        .add_item(
            track_id,
            "Solid".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 1).unwrap()).unwrap(),
            0,
        )
        .expect("item");
    let revision = service.revision().expect("revision");
    let source = service.snapshot().expect("source Project");
    let mut state = AuthoringUiState::new(timeline_id);
    state.inspector.transient_property_edit = Some(TransientPropertyEdit {
        source_revision: revision,
        owner: AuthoringPropertyOwner::Item(item_id),
        update: AuthoringPropertyValueUpdate {
            key: "opacity".to_string(),
            value: PropertyValue::from(0.25),
            target: AuthoringPropertyValueTarget::Constant,
        },
    });

    let digest = inspector_transient_edit_digest(revision, &state).expect("edit digest");
    let (projected, applied) = project_inspector_transient_edit(&source, revision, &state);

    assert_eq!(applied, Some(digest));
    assert_eq!(
        projected.items[&item_id]
            .authored_properties
            .get("opacity")
            .expect("projected opacity")
            .evaluate_at(0.0)
            .unwrap(),
        PropertyValue::from(0.25)
    );
    assert!(source.items[&item_id]
        .authored_properties
        .get("opacity")
        .is_none());
    assert_eq!(service.revision().expect("unchanged revision"), revision);
}
