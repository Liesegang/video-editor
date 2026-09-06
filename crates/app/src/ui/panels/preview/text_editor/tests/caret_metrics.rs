use super::*;

use library::core::render_plan::{evaluate_render_plan_frame, RenderPlanCompiler};
use library::editor::AuthoringPropertyOwner;

fn evaluated_frame(
    project: &AuthoringProject,
    plugins: &PluginManager,
    frame_number: u64,
) -> FrameInfo {
    let plan = RenderPlanCompiler::compile(project).expect("Text RenderPlan");
    evaluate_render_plan_frame(project, &plan, plugins, frame_number, 1.0, None)
        .expect("evaluated Text frame")
}

fn font_size(frame: &FrameInfo, item_id: TimelineItemId) -> f32 {
    item_gizmo_geometry(frame, item_id)
        .expect("Text geometry")
        .text_font_size
        .expect("evaluated Text font size")
}

#[test]
fn direct_converted_and_animated_text_share_evaluated_caret_size() {
    let fixture = TextEditorFixture::new();
    let plugins = PluginManager::default();
    fixture
        .service
        .add_appearance_operation(&plugins, fixture.first, "fill", 0)
        .expect("Fill");
    fixture
        .service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(fixture.first),
            "size".to_string(),
            PropertyValue::from(64.0),
        )
        .expect("direct Text size");
    let direct = fixture.service.snapshot().expect("direct Text Project");
    let direct_size = font_size(&evaluated_frame(&direct, &plugins, 0), fixture.first);
    assert_eq!(direct_size, 64.0);

    fixture
        .service
        .convert_source_to_node_clip(&plugins, fixture.first)
        .expect("promote Text");
    let converted = fixture.service.snapshot().expect("converted Text Project");
    assert_eq!(
        font_size(&evaluated_frame(&converted, &plugins, 0), fixture.first),
        direct_size
    );

    let content = TimelineEditorService::inspect_node_clip_text_content(&converted, fixture.first)
        .expect("inspect Node Clip Text")
        .expect("published Content");
    let definition_id = converted.module_instances[&content.instance_id].definition_id;
    let size_parameter = converted.module_definitions[&definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.target.port == "size")
        .expect("published Font Size")
        .id;
    let one_second = MediaTime::from_whole_seconds(1);
    fixture
        .service
        .upsert_module_parameter_keyframe(
            library::editor::ModuleAutomationOwner::Item(fixture.first),
            size_parameter,
            MediaTime::zero(),
            PropertyValue::from(64.0),
            None,
        )
        .expect("first size key");
    fixture
        .service
        .upsert_module_parameter_keyframe(
            library::editor::ModuleAutomationOwner::Item(fixture.first),
            size_parameter,
            one_second,
            PropertyValue::from(96.0),
            None,
        )
        .expect("animated size key");
    let animated = fixture.service.snapshot().expect("animated Text Project");
    let fps = animated.timelines[&animated.root_timeline_id].fps;
    let frame_number = fps.to_f64().round() as u64;
    let animated_size = font_size(
        &evaluated_frame(&animated, &plugins, frame_number),
        fixture.first,
    );
    assert_eq!(animated_size, 96.0);

    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0));
    assert_eq!(
        editor_font_size(
            Some(animated_size),
            canvas(egui::Vec2::ZERO, 1.5),
            screen_rect
        ),
        144.0
    );
}

#[test]
fn empty_transient_frame_retains_session_caret_metrics() {
    let fixture = TextEditorFixture::new();
    let plugins = PluginManager::default();
    fixture
        .service
        .add_appearance_operation(&plugins, fixture.first, "fill", 0)
        .expect("Fill");
    fixture
        .service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(fixture.first),
            "size".to_string(),
            PropertyValue::from(72.0),
        )
        .expect("Text size");
    let project = fixture.service.snapshot().expect("Text Project");
    let evaluated = evaluated_frame(&project, &plugins, 0);
    let geometry = item_gizmo_geometry(&evaluated, fixture.first).expect("Text geometry");
    let mut editor = crate::state::text_editor::TextEditorState::default();
    editor.begin(fixture.first, ProjectRevision::initial(), "A");
    update_editor_metrics(&mut editor, &geometry);
    let bounds = editor.layout_bounds;
    assert_eq!(editor.evaluated_font_size, Some(72.0));

    // An empty transient Text has no Item geometry. The overlay therefore
    // leaves both metrics untouched until visible glyphs return.
    let empty = TimelineEditorService::project_text(&project, fixture.first, String::new())
        .expect("empty transient Text");
    assert!(item_gizmo_geometry(&evaluated_frame(&empty, &plugins, 0), fixture.first).is_none());
    assert_eq!(editor.layout_bounds, bounds);
    assert_eq!(editor.evaluated_font_size, Some(72.0));
}
