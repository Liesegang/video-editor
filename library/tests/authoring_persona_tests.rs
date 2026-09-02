use library::TimelineEditorService;
use library::core::render_plan::RenderPlanCompiler;
use library::model::authoring::{
    BindingOperator, BindingScope, DurationPolicy, InstancePath, MatteMode, MatteRef,
    OverrideStatus, SignalBinding, SignalBindingId, SignalMapping, SignalSource, SourceRef,
    TimelineInterval,
};
use library::model::frame::color::Color;
use library::model::project::property::{PropertyValue, Vec2};
use ordered_float::OrderedFloat;

fn root(
    service: &TimelineEditorService,
) -> (
    library::model::authoring::TimelineId,
    library::model::authoring::TimelineTrackId,
) {
    let project = service.snapshot().expect("snapshot");
    let timeline_id = project.root_timeline_id;
    (timeline_id, project.timelines[&timeline_id].track_order[0])
}

#[test]
fn beginner_finishes_a_timeline_without_user_nodes() {
    let service = TimelineEditorService::create_default("Beginner").expect("project");
    let (timeline_id, track_id) = root(&service);
    let (title, _) = service
        .add_text(
            track_id,
            "Hello".to_string(),
            TimelineInterval::new(0.0, 3.0).unwrap(),
            1,
        )
        .expect("text");
    service
        .update_item_property_value(
            title,
            "position".to_string(),
            0.0,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(320.0),
                y: OrderedFloat(180.0),
            }),
        )
        .expect("direct position edit");
    service
        .trim_item(title, TimelineInterval::new(0.5, 2.0).unwrap())
        .expect("trim");
    let project = service.snapshot().expect("edited project");
    let plan = RenderPlanCompiler::compile(&project).expect("plan");
    assert!(plan.module_definitions.is_empty());
    assert_eq!(plan.timelines[&timeline_id].schedule.len(), 1);
    let (_, frame) = service
        .evaluate_frame(timeline_id, 1.0, 1.0, None)
        .expect("preview frame");
    assert_eq!(frame.object_count(), 1);
}

#[test]
fn youtuber_subtitles_and_ripple_edits_do_not_create_nodes_per_cue() {
    let service = TimelineEditorService::create_default("YouTube").expect("project");
    let (_, track_id) = root(&service);
    let (clip, _) = service
        .add_solid(
            track_id,
            Color::black(),
            TimelineInterval::new(0.0, 10.0).unwrap(),
            0,
        )
        .expect("clip");
    service.split_item(clip, 4.0).expect("split");
    let directory = tempfile::tempdir().expect("tempdir");
    let subtitles = directory.path().join("captions.srt");
    std::fs::write(
        &subtitles,
        "1\n00:00:00,500 --> 00:00:01,500\nFirst\n\n2\n00:00:02,000 --> 00:00:03,000\nSecond\n",
    )
    .expect("subtitle fixture");
    let cues = service
        .import_srt(&subtitles, track_id)
        .expect("SRT import");
    service
        .set_text(cues[0], "Corrected first subtitle".to_string())
        .expect("batch correction");
    let project = service.snapshot().expect("project");
    assert_eq!(cues.len(), 2);
    assert!(project.module_definitions.is_empty());
    assert!(project.module_instances.is_empty());
}

#[test]
fn pv_motion_uses_nested_local_time_parenting_mask_and_published_binding() {
    let service = TimelineEditorService::create_default("PV").expect("project");
    let (root_id, root_track) = root(&service);
    let (nested_id, nested_track, _) = service
        .add_timeline("Lyric".to_string(), 1280, 720, 30.0, 4.0)
        .expect("nested timeline");
    let (parent, _) = service
        .add_solid(
            nested_track,
            Color::black(),
            TimelineInterval::new(0.0, 4.0).unwrap(),
            0,
        )
        .expect("parent");
    let (text, _) = service
        .add_text(
            nested_track,
            "Lyric".to_string(),
            TimelineInterval::new(0.0, 4.0).unwrap(),
            1,
        )
        .expect("text");
    service.set_parent(text, Some(parent)).expect("parenting");
    service
        .upsert_item_keyframe(
            text,
            "position".to_string(),
            1.0,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(100.0),
                y: OrderedFloat(40.0),
            }),
            None,
        )
        .expect("motion keyframe");
    service.add_rectangle_mask(text).expect("mask");
    service
        .set_matte(
            text,
            Some(MatteRef {
                item_id: parent,
                mode: MatteMode::Alpha,
            }),
        )
        .expect("matte");
    let (instance_item, _) = service
        .place_timeline(
            root_track,
            nested_id,
            "Lyric instance".to_string(),
            TimelineInterval::new(5.0, 4.0).unwrap(),
            DurationPolicy::Fixed,
            0,
        )
        .expect("nested placement");
    let (_, frame) = service
        .evaluate_frame(root_id, 6.0, 1.0, None)
        .expect("local-time frame");
    assert!(frame.object_count() >= 2);
    assert!(matches!(
        service.snapshot().unwrap().items[&instance_item].source,
        SourceRef::Composition(_)
    ));

    let plugins = library::plugin::PluginManager::default();
    let (effect, _) = service
        .attach_effect(instance_item, "blur", &plugins)
        .expect("effect module");
    let project = service.snapshot().unwrap();
    let definition = &project.module_definitions[&project.module_instances[&effect].definition_id];
    let parameter = definition.published_parameters[0].id;
    drop(project);
    service
        .add_signal_binding(SignalBinding {
            id: SignalBindingId::new(),
            source: SignalSource::AudioEnvelope {
                channel: "music".to_string(),
            },
            scope: BindingScope::Instance {
                instance_path: InstancePath::root(root_id),
                module_instance_id: effect,
            },
            target_parameter_id: parameter,
            mapping: SignalMapping {
                input_min: OrderedFloat(0.0),
                input_max: OrderedFloat(1.0),
                output_min: OrderedFloat(0.0),
                output_max: OrderedFloat(20.0),
                clamp: true,
            },
            operator: BindingOperator::Replace,
            smoothing_seconds: OrderedFloat(0.05),
            priority: 0,
        })
        .expect("published binding");
}

#[test]
fn motion_logo_instances_keep_definition_and_overrides_separate() {
    let service = TimelineEditorService::create_default("Logo").expect("project");
    let (_, root_track) = root(&service);
    let (logo, logo_track, _) = service
        .add_timeline("Logo definition".to_string(), 800, 800, 60.0, 2.0)
        .expect("logo timeline");
    service
        .add_text(
            logo_track,
            "Brand".to_string(),
            TimelineInterval::new(0.0, 2.0).unwrap(),
            0,
        )
        .expect("logo content");
    let (first, _) = service
        .place_timeline(
            root_track,
            logo,
            "Logo A".to_string(),
            TimelineInterval::new(0.0, 3.0).unwrap(),
            DurationPolicy::Responsive {
                intro_end: OrderedFloat(0.4),
                outro_start: OrderedFloat(1.6),
            },
            0,
        )
        .expect("first instance");
    let (second, _) = service
        .place_timeline(
            root_track,
            logo,
            "Logo B".to_string(),
            TimelineInterval::new(4.0, 3.0).unwrap(),
            DurationPolicy::Loop,
            0,
        )
        .expect("second instance");
    service
        .update_item_property_value(
            first,
            "position".to_string(),
            0.0,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(50.0),
                y: OrderedFloat(25.0),
            }),
        )
        .expect("instance correction");
    let project = service.snapshot().expect("project");
    assert!(
        project.items[&second]
            .authored_properties
            .get("position")
            .is_none()
    );
    assert_eq!(project.timelines[&logo].duration, OrderedFloat(2.0));
}

#[test]
fn infographic_refresh_retains_manual_override_and_orphans_removed_row() {
    let service = TimelineEditorService::create_default("Infographic").expect("project");
    let (_, track_id) = root(&service);
    let directory = tempfile::tempdir().expect("tempdir");
    let data = directory.path().join("rows.csv");
    std::fs::write(&data, "id,text,x\nhero,Original,10\n").expect("fixture");
    let (source_id, _) = service
        .import_data_source(&data, track_id)
        .expect("data import");
    let item = *service.snapshot().unwrap().items.keys().next().unwrap();
    service
        .set_text(item, "Manual correction".to_string())
        .expect("override");
    std::fs::write(&data, "id,text,x\nhero,Updated,20\n").expect("update");
    service.refresh_data_source(source_id).expect("refresh");
    let project = service.snapshot().unwrap();
    assert!(
        matches!(&project.items[&item].source, SourceRef::Text { text } if text == "Manual correction")
    );
    assert!(
        project
            .overrides
            .values()
            .all(|authored| authored.status == OverrideStatus::Active)
    );
    drop(project);
    std::fs::write(&data, "id,text,x\n").expect("remove row");
    service.refresh_data_source(source_id).expect("refresh");
    assert!(
        service
            .snapshot()
            .unwrap()
            .overrides
            .values()
            .any(|authored| authored.status == OverrideStatus::Orphaned)
    );
}
