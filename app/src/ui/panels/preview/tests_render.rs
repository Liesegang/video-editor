#[cfg(test)]
mod render_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn preview_snapshot_releases_authoritative_lock_before_frame_evaluation() {
        let shared = Arc::new(RwLock::new(Project::new("snapshot lock boundary")));
        let snapshot = snapshot_project_for_preview(&shared);
        assert!(snapshot.is_some());
        assert!(shared.try_write().is_ok());
    }

    #[test]
    fn frame_error_is_reported_and_invalidates_stale_preview_without_dispatch() {
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.preview_texture_id = Some(42);
        editor_context.preview_texture_width = 1920;
        editor_context.preview_texture_height = 1080;
        editor_context.preview_region = Some(library::model::frame::frame::Region {
            x: 10.0,
            y: 20.0,
            width: 640.0,
            height: 360.0,
        });
        let dispatched = Cell::new(false);

        let submitted = dispatch_preview_frame(
            Err(library::LibraryError::InvalidCompositionIndex(7)),
            &mut editor_context,
            |_| dispatched.set(true),
        );

        assert!(!submitted);
        assert!(!dispatched.get());
        assert_eq!(editor_context.preview_texture_id, None);
        assert_eq!(editor_context.preview_texture_width, 0);
        assert_eq!(editor_context.preview_texture_height, 0);
        assert_eq!(editor_context.preview_region, None);
        let message = editor_context
            .interaction
            .active_modal_error
            .as_deref()
            .expect("LibraryError should reach the existing modal error path");
        assert!(message.starts_with("Failed to evaluate preview frame:"));
        assert!(message.contains('7'));
    }

    #[test]
    fn render_error_invalidates_stale_output_and_only_its_success_clears_the_modal() {
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.preview_texture_id = Some(42);
        editor_context.preview_texture_width = 1920;
        editor_context.preview_texture_height = 1080;
        editor_context.preview_nontransparent_pixels = Some(10);
        editor_context.preview_pixel_hash = Some(20);

        report_preview_render_error(
            &library::LibraryError::Render("injected shader failure".to_string()),
            &mut editor_context,
        );

        assert_eq!(editor_context.preview_texture_id, None);
        assert_eq!(editor_context.preview_texture_width, 0);
        assert_eq!(editor_context.preview_texture_height, 0);
        assert_eq!(editor_context.preview_nontransparent_pixels, None);
        assert_eq!(editor_context.preview_pixel_hash, None);
        let message = editor_context
            .interaction
            .active_modal_error
            .as_deref()
            .unwrap();
        assert!(message.starts_with(PREVIEW_RENDER_ERROR_PREFIX));
        assert!(message.contains("injected shader failure"));

        clear_preview_render_error(&mut editor_context);
        assert_eq!(editor_context.interaction.active_modal_error, None);

        editor_context.interaction.active_modal_error = Some("unrelated failure".to_string());
        clear_preview_render_error(&mut editor_context);
        assert_eq!(
            editor_context.interaction.active_modal_error.as_deref(),
            Some("unrelated failure")
        );
    }

    #[test]
    fn space_pan_owns_press_through_modifier_release_and_pointer_release() {
        let mut owner = PreviewPrimaryGesture::Idle;
        let pressed = arbitrate_primary_gesture(
            &mut owner,
            PreviewGestureInput {
                primary_pressed: true,
                primary_down: true,
                press_started_in_viewport: true,
                pan_requested: true,
                ..Default::default()
            },
        );
        assert_eq!(owner, PreviewPrimaryGesture::Pan);
        assert!(pressed.pan_owned);

        let modifier_released = arbitrate_primary_gesture(
            &mut owner,
            PreviewGestureInput {
                primary_down: true,
                primary_dragging: true,
                pan_requested: false,
                ..Default::default()
            },
        );
        assert_eq!(owner, PreviewPrimaryGesture::Pan);
        assert!(modifier_released.pan_owned);

        let released = arbitrate_primary_gesture(
            &mut owner,
            PreviewGestureInput {
                primary_released: true,
                ..Default::default()
            },
        );
        assert!(released.pan_owned);
        assert!(released.finish_after_frame);
    }

    #[test]
    fn space_can_claim_pending_press_but_not_started_content_drag() {
        let mut pending_owner = PreviewPrimaryGesture::Idle;
        arbitrate_primary_gesture(
            &mut pending_owner,
            PreviewGestureInput {
                primary_pressed: true,
                primary_down: true,
                press_started_in_viewport: true,
                ..Default::default()
            },
        );
        assert_eq!(pending_owner, PreviewPrimaryGesture::Pending);

        let claimed = arbitrate_primary_gesture(
            &mut pending_owner,
            PreviewGestureInput {
                primary_down: true,
                pan_requested: true,
                ..Default::default()
            },
        );
        assert!(claimed.pan_owned);

        let mut content_owner = PreviewPrimaryGesture::Pending;
        let started = arbitrate_primary_gesture(
            &mut content_owner,
            PreviewGestureInput {
                primary_down: true,
                primary_dragging: true,
                ..Default::default()
            },
        );
        assert!(!started.pan_owned);
        assert_eq!(content_owner, PreviewPrimaryGesture::Content);

        let modifier_changed = arbitrate_primary_gesture(
            &mut content_owner,
            PreviewGestureInput {
                primary_down: true,
                primary_dragging: true,
                pan_requested: true,
                ..Default::default()
            },
        );
        assert!(!modifier_changed.pan_owned);
        assert_eq!(content_owner, PreviewPrimaryGesture::Content);
    }

    #[test]
    fn preview_actions_edit_explicit_spatial_transform_not_text_or_output_sink() {
        use library::cache::CacheManager;
        use library::editor::project_service::ProjectManager;
        use library::model::property::{Property, PropertyValue, Vec2};
        use library::model::{Clip, Composition, NodeContainer, NodeContent};
        use library::plugin::{PluginManager, TRANSFORM_CATEGORY};
        use ordered_float::OrderedFloat;

        let plugins = Arc::new(PluginManager::default());
        let factory = ProjectManager::new(
            Arc::new(RwLock::new(Project::new("factory"))),
            plugins.clone(),
        );
        let mut graph = factory
            .create_text_graph("Text source", "Arial", 640, 360)
            .unwrap();
        let source_id = graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.content(),
                    NodeContent::Generator(library::model::GeneratorContent::Text)
                )
            })
            .unwrap()
            .id;
        let transform_id = graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.content(),
                    NodeContent::PluginOperation(operation)
                        if operation.category == TRANSFORM_CATEGORY
                )
            })
            .unwrap()
            .id;
        graph
            .nodes
            .iter_mut()
            .find(|node| node.id == transform_id)
            .unwrap()
            .set_property(
                "position".to_string(),
                Property::constant(PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(10.0),
                    y: OrderedFloat(20.0),
                })),
            )
            .expect("Transform factory initializes position");
        let sink_id = graph.output_node_id.unwrap();
        let mut model = Project::new("preview target");
        let (composition, track) = Composition::new("main", 640, 360, 30.0, 2.0);
        let track_id = track.id;
        model.add_track(track);
        model.add_composition(composition);
        let clip = Clip::new("Text Clip", 0.0, 2.0);
        let clip_id = clip.id;
        model.add_clip(clip);
        model.attach_clip_to_track(track_id, clip_id).unwrap();
        model
            .insert_node_graph(NodeContainer::Clip(clip_id), graph)
            .unwrap();
        let project = Arc::new(RwLock::new(model));
        let service = EditorService::new(
            Arc::clone(&project),
            plugins.clone(),
            Arc::new(CacheManager::new()),
        )
        .unwrap();
        let property_evaluators = plugins.get_property_evaluators();
        let evaluated = library::framing::get_frame_from_project(
            &project.read().unwrap(),
            0,
            0,
            1.0,
            None,
            &property_evaluators,
            &plugins,
        )
        .expect("fixture graph evaluates");
        let visuals = clip::from_evaluated_frame(&project.read().unwrap(), &evaluated);
        let edit_target = visuals
            .iter()
            .find(|visual| visual.spatial_id() == Some(transform_id))
            .expect("Transform visual")
            .edit_target();
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().unwrap().clone());

        let project_before_stale_write = project.read().unwrap().clone();
        let history_before_stale_write = history.undo_depth();
        let mut stale_target = edit_target.clone();
        stale_target.instance_path.push(uuid::Uuid::new_v4());
        assert!(!apply_preview_actions(
            vec![
                PreviewAction::UpdateProperty {
                    edit_target: stale_target,
                    node_id: transform_id,
                    prop_name: "position".to_string(),
                    time: 0.0,
                    value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(999.0),
                        y: OrderedFloat(999.0),
                    }),
                },
                PreviewAction::CommitHistory,
            ],
            &visuals,
            &service,
            &project,
            &mut history,
        ));
        assert_eq!(*project.read().unwrap(), project_before_stale_write);
        assert_eq!(history.undo_depth(), history_before_stale_write);

        assert!(!apply_preview_actions(
            vec![PreviewAction::CommitHistory],
            &visuals,
            &service,
            &project,
            &mut history,
        ));
        assert_eq!(history.undo_depth(), 1, "a no-output frame is not an edit");

        assert!(apply_preview_actions(
            vec![
                PreviewAction::UpdateProperty {
                    edit_target,
                    node_id: transform_id,
                    prop_name: "position".to_string(),
                    time: 0.0,
                    value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(30.0),
                        y: OrderedFloat(40.0),
                    }),
                },
                PreviewAction::CommitHistory,
            ],
            &visuals,
            &service,
            &project,
            &mut history,
        ));

        let model = project.read().unwrap();
        assert_eq!(
            model
                .get_node(transform_id)
                .unwrap()
                .properties()
                .get("position")
                .and_then(Property::value),
            Some(&PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(30.0),
                y: OrderedFloat(40.0),
            }))
        );
        assert!(
            model
                .get_node(source_id)
                .unwrap()
                .properties()
                .get("position")
                .is_none(),
            "Text content ownership must remain independent from spatial edits"
        );
        assert!(
            model
                .get_node(sink_id)
                .unwrap()
                .properties()
                .get("position")
                .is_none(),
            "the output sink must not receive a guessed transform property"
        );
        assert_eq!(history.undo_depth(), 2);
    }
}
