use super::*;

use library::core::render_plan::{evaluate_render_plan_frame, RenderPlanCompiler};
use library::model::authoring::RationalRate;

fn converted_fixture() -> TextEditorFixture {
    let mut fixture = TextEditorFixture::new();
    let plugins = PluginManager::default();
    fixture
        .service
        .add_appearance_operation(&plugins, fixture.first, "fill", 0)
        .unwrap();
    fixture
        .service
        .convert_source_to_node_clip(&plugins, fixture.first)
        .unwrap();
    fixture.state.preview.text_editor.finish();
    fixture
}

fn start_edit(fixture: &mut TextEditorFixture) {
    let (project, revision) = fixture.current_project();
    let content = resolve_text(&project, &fixture.state, fixture.first)
        .unwrap()
        .unwrap();
    begin_edit(&mut fixture.state, fixture.first, revision, content);
    fixture.state.preview.text_editor.layout_bounds = Some(LAYOUT_BOUNDS);
}

#[test]
fn converted_text_draft_and_accept_use_the_same_public_instance_value() {
    let mut fixture = converted_fixture();
    start_edit(&mut fixture);
    let (before, revision) = fixture.current_project();
    fixture.state.preview.text_editor.buffer = "Canvas content".to_string();
    let (projected, digest) = transient_render_project(&before, revision, &fixture.state).unwrap();
    assert!(digest.is_some());
    assert_eq!(fixture.current_project(), (Arc::clone(&before), revision));
    assert_eq!(projected.module_definitions, before.module_definitions);
    assert_eq!(projected.items, before.items);
    assert_eq!(
        resolve_text(&projected, &fixture.state, fixture.first)
            .unwrap()
            .unwrap()
            .text,
        "Canvas content"
    );
    assert!(accept_if_active(&mut fixture.state, &fixture.service));
    let (committed, committed_revision) = fixture.current_project();
    assert_eq!(committed, projected);
    assert_eq!(committed_revision.get(), revision.get() + 1);
    fixture.service.undo().unwrap();
    assert_eq!(fixture.service.snapshot().unwrap(), before);
}

#[test]
fn converted_content_automation_edits_the_started_local_keyframe_without_clearing_others() {
    let mut fixture = converted_fixture();
    let mut placed = fixture.current_project().0.as_ref().clone();
    let item = placed.items.get_mut(&fixture.first).unwrap();
    item.interval.start = MediaTime::from_whole_seconds(2);
    item.time_map.source_start = MediaTime::from_whole_seconds(1);
    item.time_map.playback_rate = RationalRate::new(2, 1).unwrap();
    fixture.service = TimelineEditorService::new(placed).unwrap();
    let (project, _) = fixture.current_project();
    let content = TimelineEditorService::inspect_node_clip_text_content(&project, fixture.first)
        .unwrap()
        .unwrap();
    let first_time = MediaTime::from_whole_seconds(1);
    let second_time = MediaTime::from_whole_seconds(3);
    let (first_key, _) = fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.first,
            content.parameter_id,
            first_time,
            PropertyValue::String("First".to_string()),
            None,
        )
        .unwrap();
    let (second_key, _) = fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.first,
            content.parameter_id,
            second_time,
            PropertyValue::String("Later".to_string()),
            None,
        )
        .unwrap();
    let fps = project.timelines[&fixture.state.active_timeline_id].fps;
    // Use the Timeline's rate rather than assuming the default fixture fps.
    fixture.state.timeline.current_frame = (fps.to_f64() * 3.0).round() as i64;
    start_edit(&mut fixture);
    assert_eq!(fixture.state.preview.text_editor.original, "Later");
    assert!(matches!(
        fixture
            .state
            .preview
            .text_editor
            .parameter_target
            .unwrap()
            .value_target,
        AuthoringPropertyValueTarget::Keyframe { local_time, .. } if local_time == second_time
    ));
    let (before, revision) = fixture.current_project();
    fixture.state.preview.text_editor.buffer = "Edited later".to_string();
    let (projected, _) = transient_render_project(&before, revision, &fixture.state).unwrap();
    assert!(accept_if_active(&mut fixture.state, &fixture.service));
    let committed = fixture.service.snapshot().unwrap();
    assert_eq!(committed, projected);
    assert_eq!(committed.module_instances, before.module_instances);
    assert_eq!(committed.module_definitions, before.module_definitions);
    let SourceRef::Module(invocation) = &committed.items[&fixture.first].source else {
        panic!()
    };
    let track = &invocation.automation_tracks[&content.parameter_id];
    assert_eq!(track.keyframes.len(), 2);
    assert_eq!(track.keyframes[0].id, first_key);
    assert_eq!(track.keyframes[1].id, second_key);
    assert_eq!(
        track.evaluate_at(first_time).unwrap(),
        PropertyValue::String("First".to_string())
    );
    assert_eq!(
        track.evaluate_at(second_time).unwrap(),
        PropertyValue::String("Edited later".to_string())
    );
    fixture.service.undo().unwrap();
    assert_eq!(fixture.service.snapshot().unwrap(), before);
}

#[test]
fn converted_typing_reserves_one_new_key_for_the_entire_canvas_session() {
    let mut fixture = converted_fixture();
    let (project, _) = fixture.current_project();
    let content = TimelineEditorService::inspect_node_clip_text_content(&project, fixture.first)
        .unwrap()
        .unwrap();
    fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.first,
            content.parameter_id,
            MediaTime::zero(),
            PropertyValue::String("Start".into()),
            None,
        )
        .unwrap();
    let fps = project.timelines[&fixture.state.active_timeline_id].fps;
    fixture.state.timeline.current_frame = fps.to_f64().round() as i64;
    start_edit(&mut fixture);
    let value_target = fixture
        .state
        .preview
        .text_editor
        .parameter_target
        .unwrap()
        .value_target;
    let AuthoringPropertyValueTarget::Keyframe {
        local_time,
        insertion_id,
    } = value_target
    else {
        panic!()
    };
    assert_eq!(local_time, MediaTime::from_whole_seconds(1));
    let (before, revision) = fixture.current_project();
    for text in ["Draft one", "Draft two", "Final"] {
        fixture.state.preview.text_editor.buffer = text.into();
        let (projected, _) = transient_render_project(&before, revision, &fixture.state).unwrap();
        let SourceRef::Module(invocation) = &projected.items[&fixture.first].source else {
            panic!()
        };
        let keys = &invocation.automation_tracks[&content.parameter_id].keyframes;
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[1].id, insertion_id);
        assert_eq!(keys[1].value, PropertyValue::String(text.into()));
        assert_eq!(fixture.current_project(), (Arc::clone(&before), revision));
    }
    let (projected, _) = transient_render_project(&before, revision, &fixture.state).unwrap();
    assert!(accept_if_active(&mut fixture.state, &fixture.service));
    assert_eq!(fixture.service.snapshot().unwrap(), projected);
    assert_eq!(
        fixture.service.revision().unwrap().get(),
        revision.get() + 1
    );
    fixture.service.undo().unwrap();
    assert_eq!(fixture.service.snapshot().unwrap(), before);
    start_edit(&mut fixture);
    assert_ne!(
        fixture
            .state
            .preview
            .text_editor
            .parameter_target
            .unwrap()
            .value_target,
        value_target
    );
    fixture.state.preview.text_editor.finish();
    assert_eq!(fixture.service.snapshot().unwrap(), before);
    fixture.service.redo().unwrap();
    assert_eq!(fixture.service.snapshot().unwrap(), projected);
}

#[test]
fn converted_typing_context_changes_cancel_without_committing_at_a_different_time_or_path() {
    for change_path in [false, true] {
        let mut fixture = converted_fixture();
        start_edit(&mut fixture);
        let (before, revision) = fixture.current_project();
        fixture.state.preview.text_editor.buffer = "Must not commit".to_string();
        if change_path {
            fixture.state.active_instance_path = None;
        } else {
            fixture.state.timeline.current_frame += 1;
        }
        assert!(transient_edit_digest(&fixture.state, revision).is_none());
        assert!(!accept_if_active(&mut fixture.state, &fixture.service));
        assert_eq!(fixture.current_project(), (before, revision));
        assert!(!fixture.state.preview.text_editor.editing);
    }
}

#[test]
fn text_tool_hits_converted_text_but_does_not_create_over_unpublished_text() {
    for published in [true, false] {
        let mut fixture = converted_fixture();
        let track_id = fixture.current_project().0.items[&fixture.second].track_id;
        fixture
            .service
            .move_item(
                fixture.second,
                track_id,
                MediaTime::from_whole_seconds(5),
                1,
            )
            .unwrap();
        let (source, _) = fixture.current_project();
        if !published {
            let content =
                TimelineEditorService::inspect_node_clip_text_content(&source, fixture.first)
                    .unwrap()
                    .unwrap();
            let mut project = source.as_ref().clone();
            let instance = project
                .module_instances
                .get_mut(&content.instance_id)
                .unwrap();
            instance.parameter_overrides.remove(&content.parameter_id);
            project
                .module_definitions
                .get_mut(&instance.definition_id)
                .unwrap()
                .interface
                .parameters
                .retain(|parameter| parameter.id != content.parameter_id);
            fixture.service = TimelineEditorService::new(project).unwrap();
        }
        let (before, revision) = fixture.current_project();
        let plugins = PluginManager::default();
        let plan = RenderPlanCompiler::compile(&before).unwrap();
        let frame = evaluate_render_plan_frame(&before, &plan, &plugins, 0, 1.0, None).unwrap();
        let geometry = item_gizmo_geometry(&frame, fixture.first).unwrap();
        let transform = canvas(egui::Vec2::ZERO, 1.0);
        let point = transform.world_to_screen(geometry.local_bounds.center());
        assert!(VIEWPORT.contains(point));
        let context = egui::Context::default();
        fixture.run_click(
            &context,
            transform,
            Some(&frame),
            vec![egui::Event::PointerMoved(point)],
        );
        fixture.run_click(
            &context,
            transform,
            Some(&frame),
            vec![pointer_button_at(point, true)],
        );
        fixture.run_click(
            &context,
            transform,
            Some(&frame),
            vec![pointer_button_at(point, false)],
        );
        assert_eq!(fixture.current_project(), (before, revision));
        assert_eq!(
            fixture.state.selection.primary(),
            Some(AuthoringSelection::Item(fixture.first))
        );
        assert_eq!(fixture.state.preview.text_editor.editing, published);
        if published {
            assert_eq!(
                fixture.state.preview.text_editor.target_item,
                Some(fixture.first)
            );
            assert_eq!(fixture.state.preview.text_editor.buffer, "A");
        } else {
            assert!(fixture.state.status.contains("no single editable Text"));
        }
    }
}
