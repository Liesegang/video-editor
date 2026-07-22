use super::*;

#[test]
fn moving_composition_moves_track_clip_and_nodes_without_changing_containment() {
    let (mut project, composition_id, track_id, clip_id, solid_id, _) = fixture();
    let original_clip_ids = project.get_track(track_id).unwrap().clip_ids.clone();
    let original_node_ids = project.get_clip(clip_id).unwrap().node_ids.clone();

    assert!(translate_container(
        &mut project,
        PortOwner::Composition(composition_id),
        [25.0, -15.0]
    ));

    assert_eq!(
        project.get_track(track_id).unwrap().ui_position,
        [135.0, 125.0]
    );
    assert_eq!(
        project.get_clip(clip_id).unwrap().ui_position,
        [285.0, 245.0]
    );
    assert_eq!(
        project.get_node(solid_id).unwrap().ui_position,
        [475.0, 375.0]
    );
    assert_eq!(
        project.get_track(track_id).unwrap().clip_ids,
        original_clip_ids
    );
    assert_eq!(
        project.get_clip(clip_id).unwrap().node_ids,
        original_node_ids
    );
    assert_eq!(project.connections.len(), 6);
    assert!(project.validate_connections().is_empty());
}

#[test]
fn clip_resize_and_timing_edits_persist_on_the_clip_only() {
    let (mut project, _, _, clip_id, solid_id, _) = fixture();
    assert!(set_container_size(
        &mut project,
        PortOwner::Clip(clip_id),
        [720.0, 520.0]
    ));
    assert!(apply_edit(
        &mut project,
        NodeEdit::SetProperty {
            owner: PortOwner::Clip(clip_id),
            key: "start_time".into(),
            time: 0.0,
            value: PropertyValue::Number(OrderedFloat(2.5)),
        }
    ));
    assert_eq!(project.get_clip(clip_id).unwrap().ui_size, [720.0, 520.0]);
    assert_eq!(
        project.get_clip(clip_id).unwrap().start_time.into_inner(),
        2.5
    );
    assert!(project.get_node(solid_id).is_some());
}

#[test]
fn clip_owned_node_properties_use_local_time_for_evaluation_and_keyframe_edits() {
    let (mut project, composition_id, _, clip_id, solid_id, _) = fixture();
    {
        let clip = project.get_clip_mut(clip_id).unwrap();
        clip.start_time = OrderedFloat(4.0);
        clip.duration = OrderedFloat(10.0);
        clip.trim_in = OrderedFloat(1.25);
        clip.time_stretch = OrderedFloat(1.5);
    }
    let animated = Property::keyframe(vec![
        Keyframe::new(
            0.0,
            PropertyValue::Number(OrderedFloat(0.0)),
            EasingFunction::Linear,
        ),
        Keyframe::new(
            10.0,
            PropertyValue::Number(OrderedFloat(100.0)),
            EasingFunction::Linear,
        ),
    ]);
    project
        .get_node_mut(solid_id)
        .unwrap()
        .set_property("opacity".to_string(), animated.clone())
        .expect("solid factory initializes opacity");

    let global_time = 6.0;
    let inspector_and_renderer_time = project.get_clip(clip_id).unwrap().local_time(global_time);
    assert_eq!(inspector_and_renderer_time, 4.25);
    assert_eq!(
        node_property_time(&project, None, solid_id, global_time),
        inspector_and_renderer_time
    );
    assert_eq!(
        project
            .get_node(solid_id)
            .unwrap()
            .properties()
            .get("opacity")
            .unwrap()
            .evaluate_at(node_property_time(&project, None, solid_id, global_time))
            .unwrap(),
        PropertyValue::Number(OrderedFloat(42.5))
    );

    assert!(apply_edit(
        &mut project,
        NodeEdit::SetProperty {
            owner: PortOwner::Node(solid_id),
            key: "opacity".into(),
            time: inspector_and_renderer_time,
            value: PropertyValue::Number(OrderedFloat(91.0)),
        }
    ));
    let clip_node_property = project
        .get_node(solid_id)
        .unwrap()
        .properties()
        .get("opacity")
        .unwrap();
    assert_eq!(
        clip_node_property
            .evaluate_at(inspector_and_renderer_time)
            .unwrap(),
        PropertyValue::Number(OrderedFloat(91.0))
    );
    assert!(clip_node_property.has_keyframe_at(inspector_and_renderer_time, 0.001));
    assert!(!clip_node_property.has_keyframe_at(global_time, 0.001));

    let root_id = Uuid::from_u128(0x9_101);
    let mut root = PluginManager::default()
        .create_style_operation_node("fill")
        .expect("Fill descriptor is valid");
    root.name = "Root".to_string();
    root.id = root_id;
    root.set_property("opacity".to_string(), animated)
        .expect("Fill factory initializes opacity");
    project.add_node(root);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), root_id)
        .unwrap();
    assert_eq!(
        node_property_time(&project, None, root_id, global_time),
        global_time,
        "Composition-owned Node time stays in the global domain"
    );
    let root_property_time = node_property_time(&project, None, root_id, global_time);
    assert!(apply_edit(
        &mut project,
        NodeEdit::SetProperty {
            owner: PortOwner::Node(root_id),
            key: "opacity".into(),
            time: root_property_time,
            value: PropertyValue::Number(OrderedFloat(55.0)),
        }
    ));
    let root_property = project
        .get_node(root_id)
        .unwrap()
        .properties()
        .get("opacity")
        .unwrap();
    assert!(root_property.has_keyframe_at(global_time, 0.001));
    assert_eq!(
        root_property.evaluate_at(global_time).unwrap(),
        PropertyValue::Number(OrderedFloat(55.0))
    );
}

#[test]
fn numeric_drag_text_typing_and_color_popup_each_commit_one_undoable_gesture() {
    let (mut numeric_project, _, _, clip_id, solid_id, _) = fixture();
    {
        let clip = numeric_project.get_clip_mut(clip_id).unwrap();
        clip.start_time = OrderedFloat(4.0);
        clip.trim_in = OrderedFloat(1.25);
        clip.time_stretch = OrderedFloat(1.5);
    }
    let numeric_initial = numeric_project.clone();
    let numeric_time = node_property_time(&numeric_project, None, solid_id, 6.0);
    assert_eq!(numeric_time, 4.25);
    let mut numeric_history = HistoryManager::new();
    numeric_history.push_project_state(numeric_initial.clone());
    let mut numeric_state = NodeEditorState::default();
    for value in [10.0, 20.0, 30.0] {
        assert!(apply_queued_node_edits(
            &mut numeric_project,
            vec![queued_property_edit(
                PortOwner::Node(solid_id),
                "opacity",
                numeric_time,
                PropertyValue::Number(OrderedFloat(value)),
                false,
            )],
            &mut numeric_history,
            &mut numeric_state,
        ));
        assert_eq!(numeric_history.undo_depth(), 1);
    }
    assert!(!apply_queued_node_edits(
        &mut numeric_project,
        vec![queued_finish(PortOwner::Node(solid_id), "opacity")],
        &mut numeric_history,
        &mut numeric_state,
    ));
    assert!(numeric_state.pending_continuous_edit.is_none());
    let numeric_edited = numeric_project.clone();
    assert_single_gesture_undo_redo(&mut numeric_history, &numeric_initial, &numeric_edited);

    let (mut text_project, _, _, _, text_node_id, _) = fixture();
    let text_initial = text_project.clone();
    let mut text_history = HistoryManager::new();
    text_history.push_project_state(text_initial.clone());
    let mut text_state = NodeEditorState::default();
    for name in ["S", "So", "Solid renamed"] {
        assert!(apply_queued_node_edits(
            &mut text_project,
            vec![QueuedNodeEdit::Continuous {
                pending: NodeEditorPendingEdit {
                    owner: PortOwner::Node(text_node_id),
                    key: "$name".into(),
                },
                edit: Some(NodeEdit::Rename {
                    node_id: text_node_id,
                    name: name.into(),
                }),
                finished: false,
            }],
            &mut text_history,
            &mut text_state,
        ));
        assert_eq!(text_history.undo_depth(), 1);
    }
    apply_queued_node_edits(
        &mut text_project,
        vec![queued_finish(PortOwner::Node(text_node_id), "$name")],
        &mut text_history,
        &mut text_state,
    );
    let text_edited = text_project.clone();
    assert_single_gesture_undo_redo(&mut text_history, &text_initial, &text_edited);

    let (mut color_project, _, _, _, color_node_id, _) = fixture();
    let color_initial = color_project.clone();
    let mut color_history = HistoryManager::new();
    color_history.push_project_state(color_initial.clone());
    let mut color_state = NodeEditorState::default();
    for color in [
        library::model::frame::color::Color {
            r: 20,
            g: 30,
            b: 40,
            a: 255,
        },
        library::model::frame::color::Color {
            r: 80,
            g: 90,
            b: 100,
            a: 220,
        },
    ] {
        assert!(apply_queued_node_edits(
            &mut color_project,
            vec![queued_property_edit(
                PortOwner::Node(color_node_id),
                "color",
                0.0,
                PropertyValue::Color(color),
                false,
            )],
            &mut color_history,
            &mut color_state,
        ));
        assert_eq!(color_history.undo_depth(), 1);
    }
    apply_queued_node_edits(
        &mut color_project,
        vec![queued_finish(PortOwner::Node(color_node_id), "color")],
        &mut color_history,
        &mut color_state,
    );
    let color_edited = color_project.clone();
    assert_single_gesture_undo_redo(&mut color_history, &color_initial, &color_edited);
}

#[test]
fn owner_or_control_switch_flushes_previous_edit_and_atomic_checkbox_commits_immediately() {
    let (mut project, _, _, _, node_id, _) = fixture();
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial);
    let mut state = NodeEditorState::default();

    apply_queued_node_edits(
        &mut project,
        vec![queued_property_edit(
            PortOwner::Node(node_id),
            "opacity",
            0.0,
            PropertyValue::Number(OrderedFloat(25.0)),
            false,
        )],
        &mut history,
        &mut state,
    );
    let after_numeric = project.clone();
    assert_eq!(history.undo_depth(), 1);

    apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Continuous {
            pending: NodeEditorPendingEdit {
                owner: PortOwner::Node(node_id),
                key: "$name".into(),
            },
            edit: Some(NodeEdit::Rename {
                node_id,
                name: "switched control".into(),
            }),
            finished: false,
        }],
        &mut history,
        &mut state,
    );
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(history.undo(&project), Some(after_numeric.clone()));
    assert_eq!(history.redo(&after_numeric), Some(project.clone()));

    let before_owner_switch = project.clone();
    let project_lock = Arc::new(RwLock::new(project));
    assert!(flush_pending_continuous_edit(
        &project_lock,
        &mut history,
        &mut state,
    ));
    assert_eq!(history.undo_depth(), 3);
    assert!(state.pending_continuous_edit.is_none());
    assert_eq!(
        history.undo(&before_owner_switch),
        Some(after_numeric.clone())
    );
    assert_eq!(
        history.redo(&after_numeric),
        Some(before_owner_switch.clone())
    );

    let mut project = project_lock.read().unwrap().clone();
    apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
            node_id,
            enabled: false,
        })],
        &mut history,
        &mut state,
    );
    assert_eq!(history.undo_depth(), 4);
    assert!(state.pending_continuous_edit.is_none());
}

#[test]
fn zero_time_stretch_is_preserved_as_freeze_and_negative_input_is_rejected() {
    let (mut project, _, _, clip_id, _, _) = fixture();
    {
        let clip = project.get_clip_mut(clip_id).unwrap();
        clip.trim_in = OrderedFloat(2.25);
        clip.time_stretch = OrderedFloat(0.0);
    }
    let serialized = serde_json::to_string(&project).unwrap();
    let mut loaded: Project = serde_json::from_str(&serialized).unwrap();
    let loaded_value = loaded.get_clip(clip_id).unwrap().time_stretch.into_inner();
    assert_eq!(
        Clip::validate_timing_property_value(
            "time_stretch",
            &PropertyValue::Number(OrderedFloat(loaded_value)),
        )
        .unwrap(),
        0.0
    );
    assert!(Clip::validate_timing_property_value(
        "time_stretch",
        &PropertyValue::Number(OrderedFloat(-0.5)),
    )
    .is_err());

    assert!(apply_edit(
        &mut loaded,
        NodeEdit::SetProperty {
            owner: PortOwner::Clip(clip_id),
            key: "time_stretch".into(),
            time: 9.0,
            value: PropertyValue::Number(OrderedFloat(loaded_value)),
        }
    ));
    let clip = loaded.get_clip(clip_id).unwrap();
    assert_eq!(clip.time_stretch, OrderedFloat(0.0));
    assert_eq!(clip.local_time(clip.start_time.into_inner()), 2.25);
    assert_eq!(clip.local_time(clip.start_time.into_inner() + 100.0), 2.25);
}

#[test]
fn deleting_a_clip_removes_only_its_owned_leaf_nodes() {
    let (mut project, _, track_id, clip_id, solid_id, merge_id) = fixture();
    assert!(apply_edit(
        &mut project,
        NodeEdit::Delete {
            owner: PortOwner::Clip(clip_id),
        }
    ));
    assert!(project.get_clip(clip_id).is_none());
    assert!(project.get_node(solid_id).is_none());
    assert!(project.get_node(merge_id).is_none());
    assert!(project.get_track(track_id).is_some());
    let track_merge_id = project
        .get_track(track_id)
        .unwrap()
        .structural_merge_node_id;
    let track_sound_merge_id = project
        .get_track(track_id)
        .unwrap()
        .structural_sound_merge_node_id;
    assert_eq!(project.connections.len(), 2);
    assert!(project.connections.iter().all(|connection| {
        connection.from.owner == PortOwner::Track(track_id)
            || matches!(
                connection.to.owner,
                PortOwner::Node(id) if id == track_merge_id || id == track_sound_merge_id
            )
    }));
    assert!(project.validate_connections().is_empty());
}
