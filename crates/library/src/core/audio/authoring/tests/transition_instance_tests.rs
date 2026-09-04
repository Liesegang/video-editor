use super::*;

fn add_root_composition_placement(
    project: &mut AuthoringProject,
    track_id: TimelineTrackId,
    nested_timeline_id: TimelineId,
    start_frame: i64,
    layer: i64,
) -> TimelineItemId {
    let id = TimelineItemId::new();
    project.items.insert(
        id,
        TimelineItem {
            id,
            track_id,
            name: format!("Nested {layer}"),
            source: SourceRef::Composition(CompositionInstance {
                timeline_id: nested_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            interval: TimelineInterval::new(frame_time(start_frame), frame_time(12)).unwrap(),
            time_map: TimeMap::default(),
            layer,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    id
}

#[test]
fn audio_transition_parameter_uses_the_concrete_nested_instance_path() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("instance-from.wav");
    let to_path = directory.path().join("instance-to.wav");
    write_stereo_wave(&from_path, &[[0.25; 2]; 12]);
    write_stereo_wave(&to_path, &[[0.75; 2]; 12]);

    let mut project = project_with_audio_track(12);
    let nested_timeline_id = project.root_timeline_id;
    let from_asset = add_audio_asset(&mut project, &from_path, 12);
    let to_asset = add_audio_asset(&mut project, &to_path, 12);
    let nested_track_id = project.timelines[&nested_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, nested_track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, nested_track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    let definition_id = promote_audio_crossfade_to_module(&mut project, transition_id);
    let parameter_id = publish_audio_mix_progress(&mut project, definition_id);

    let root_timeline_id = TimelineId::new();
    let root_track_id = TimelineTrackId::new();
    project.root_timeline_id = root_timeline_id;
    project.timelines.insert(
        root_timeline_id,
        Timeline {
            id: root_timeline_id,
            name: "Song".to_string(),
            width: 64,
            height: 64,
            fps: RationalRate::new(24, 1).unwrap(),
            duration: frame_time(32),
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            track_order: vec![root_track_id],
            authored_properties: PropertyMap::new(),
            published_parameters: Vec::new(),
        },
    );
    project.tracks.insert(
        root_track_id,
        TimelineTrack {
            id: root_track_id,
            timeline_id: root_timeline_id,
            name: "Nested audio".to_string(),
            kind: TimelineTrackKind::Audio,
            authored_properties: PropertyMap::new(),
        },
    );
    let first_item =
        add_root_composition_placement(&mut project, root_track_id, nested_timeline_id, 0, 0);
    let _second_item =
        add_root_composition_placement(&mut project, root_track_id, nested_timeline_id, 20, 1);
    project.validate().unwrap();

    let service = crate::editor::TimelineEditorService::new(project).unwrap();
    let first_path = InstancePath::root(root_timeline_id).nested(first_item);
    service
        .set_transition_module_instance_parameter(
            &first_path,
            transition_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(1.0)),
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    let cache = CacheManager::with_audio_chunk_capacity(4);
    let mut mixer = AuthoringAudioMixer::root(&project, &cache).unwrap();

    let first = mixer.render_window(5, 1).unwrap();
    let second = mixer.render_window(25, 1).unwrap();
    assert_stereo_near(frame(&first, 0), [0.75; 2]);
    assert_stereo_near(frame(&second, 0), [0.25; 2]);
}
