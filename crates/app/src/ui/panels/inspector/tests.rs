use super::*;

fn scroll_frame(context: &egui::Context, frame: usize, events: Vec<egui::Event>) -> egui::Vec2 {
    let mut offset = egui::Vec2::ZERO;
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(240.0, 160.0),
            )),
            time: Some(frame as f64 / 60.0),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let output = inspector_scroll_area().show(ui, |ui| {
                    ui.allocate_space(egui::vec2(120.0, 640.0));
                });
                offset = output.state.offset;
            });
        },
    ));
    offset
}

#[test]
fn inspector_blank_drag_does_not_scroll_but_wheel_and_scrollbar_remain_enabled() {
    assert_eq!(
        INSPECTOR_SCROLL_SOURCE,
        egui::containers::scroll_area::ScrollSource {
            scroll_bar: true,
            drag: false,
            mouse_wheel: true,
        }
    );

    let context = egui::Context::default();
    let point = egui::pos2(80.0, 80.0);
    assert_eq!(
        scroll_frame(&context, 0, vec![egui::Event::PointerMoved(point)],),
        egui::Vec2::ZERO
    );
    assert_eq!(
        scroll_frame(
            &context,
            1,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        ),
        egui::Vec2::ZERO
    );
    assert_eq!(
        scroll_frame(
            &context,
            2,
            vec![egui::Event::PointerMoved(point - egui::vec2(0.0, 50.0))],
        ),
        egui::Vec2::ZERO
    );
    assert_eq!(
        scroll_frame(
            &context,
            3,
            vec![egui::Event::PointerButton {
                pos: point - egui::vec2(0.0, 50.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ),
        egui::Vec2::ZERO
    );

    let wheel_offset = scroll_frame(
        &context,
        4,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -50.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(
        wheel_offset.y > 0.0,
        "mouse wheel must still scroll Inspector"
    );
}

#[test]
fn seeking_refreshes_only_time_dependent_inspector_drafts() {
    let service = TimelineEditorService::create_default("Inspector").expect("service");
    let project = service.snapshot().expect("project");
    let revision = service.revision().expect("revision");
    let timeline_id = project.root_timeline_id;
    let selection = Some(AuthoringSelection::Timeline(timeline_id));
    let mut state = AuthoringUiState::new(timeline_id);
    sync_draft(&project, &mut state, selection, revision);

    state.inspector.name = "name edit in progress".to_string();
    state.inspector.property_values.insert(
        "source:text".to_string(),
        PropertyValue::String("text edit in progress".to_string()),
    );
    state
        .inspector
        .property_values
        .insert("authored:opacity".to_string(), PropertyValue::from(0.25));
    state.inspector.effect_values.insert(
        (
            library::model::authoring::AttachmentId::new(),
            "sigma_x".to_string(),
        ),
        PropertyValue::from(10.0),
    );
    state
        .inspector
        .expression_sources
        .insert("item:test:opacity".to_string(), "value * 2".to_string());

    state.timeline.current_frame = 1;
    sync_draft(&project, &mut state, selection, revision);

    assert_eq!(
        state.inspector.property_values.get("source:text"),
        Some(&PropertyValue::String("text edit in progress".to_string()))
    );
    assert!(!state
        .inspector
        .property_values
        .contains_key("authored:opacity"));
    assert!(state.inspector.effect_values.is_empty());
    assert_eq!(state.inspector.name, "name edit in progress");
    assert_eq!(
        state.inspector.expression_sources["item:test:opacity"],
        "value * 2"
    );
    assert_eq!(
        state.inspector.synced_context,
        Some(state.preview_edit_context())
    );
}

#[test]
fn escape_cancels_the_shared_property_projection_and_egui_drag() {
    let timeline_id = library::model::authoring::TimelineId::new();
    let item_id = library::model::authoring::TimelineItemId::new();
    let mut state = AuthoringUiState::new(timeline_id);
    state
        .inspector
        .property_values
        .insert("authored:position".to_string(), PropertyValue::from(25.0));
    state.inspector.transient_property_edit =
        Some(crate::state::authoring::TransientPropertyEdit::authored(
            library::model::authoring::ProjectRevision::initial(),
            AuthoringPropertyOwner::Item(item_id),
            library::editor::AuthoringPropertyValueUpdate {
                key: "position".to_string(),
                value: PropertyValue::from(25.0),
                target: library::editor::AuthoringPropertyValueTarget::Constant,
            },
        ));
    let context = egui::Context::default();
    let dragged_id = egui::Id::new("inspector-property-drag");
    let mut cancelled = false;
    drop(context.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |context| {
            context.set_dragged_id(dragged_id);
            egui::CentralPanel::default().show(context, |ui| {
                cancelled = cancel_transient_property_edit(ui, &mut state);
            });
        },
    ));

    assert!(cancelled);
    assert!(context.dragged_id().is_none());
    assert!(state.inspector.transient_property_edit.is_none());
    assert!(state.inspector.property_values.is_empty());
    assert_eq!(state.status, "Cancelled property edit");
}

#[test]
fn playhead_change_discards_projection_and_stops_the_held_drag() {
    let service = TimelineEditorService::create_default("Inspector context").unwrap();
    let project = service.snapshot().unwrap();
    let revision = service.revision().unwrap();
    let timeline_id = project.root_timeline_id;
    let item_id = library::model::authoring::TimelineItemId::new();
    let selection = Some(AuthoringSelection::Timeline(timeline_id));
    let mut state = AuthoringUiState::new(timeline_id);
    assert!(!sync_draft(&project, &mut state, selection, revision));
    state.inspector.transient_property_edit =
        Some(crate::state::authoring::TransientPropertyEdit::authored(
            revision,
            AuthoringPropertyOwner::Item(item_id),
            library::editor::AuthoringPropertyValueUpdate {
                key: "position".to_string(),
                value: PropertyValue::from(25.0),
                target: library::editor::AuthoringPropertyValueTarget::Constant,
            },
        ));
    state
        .inspector
        .property_values
        .insert("authored:position".to_string(), PropertyValue::from(25.0));
    state.timeline.current_frame = 1;

    let context = egui::Context::default();
    let dragged_id = egui::Id::new("inspector-context-drag");
    drop(context.run(egui::RawInput::default(), |context| {
        context.set_dragged_id(dragged_id);
        egui::CentralPanel::default().show(context, |ui| {
            let discarded = sync_draft(&project, &mut state, selection, revision);
            assert!(discarded);
            stop_discarded_property_drag(ui, discarded);
        });
    }));

    assert!(context.dragged_id().is_none());
    assert!(state.inspector.transient_property_edit.is_none());
    assert!(!state
        .inspector
        .property_values
        .contains_key("authored:position"));

    state.inspector.transient_property_edit =
        Some(crate::state::authoring::TransientPropertyEdit::authored(
            revision,
            AuthoringPropertyOwner::Item(item_id),
            library::editor::AuthoringPropertyValueUpdate {
                key: "position".to_string(),
                value: PropertyValue::from(50.0),
                target: library::editor::AuthoringPropertyValueTarget::Constant,
            },
        ));
    state.active_instance_path = Some(
        library::model::authoring::InstancePath::root(timeline_id)
            .nested(library::model::authoring::TimelineItemId::new()),
    );
    drop(context.run(egui::RawInput::default(), |context| {
        context.set_dragged_id(dragged_id);
        egui::CentralPanel::default().show(context, |ui| {
            let discarded = sync_draft(&project, &mut state, selection, revision);
            assert!(discarded);
            stop_discarded_property_drag(ui, discarded);
        });
    }));
    assert!(context.dragged_id().is_none());
    assert!(state.inspector.transient_property_edit.is_none());
}

#[test]
fn stale_frame_snapshot_keeps_its_paired_revision_and_cannot_overwrite() {
    let service = TimelineEditorService::create_default("Inspector paired revision").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let (item_id, _) = service
        .add_item(
            track_id,
            "Solid".to_string(),
            SourceRef::Solid {
                color: library::model::frame::color::Color::black(),
            },
            library::model::authoring::TimelineInterval::new(
                MediaTime::zero(),
                MediaTime::new(2, 1).unwrap(),
            )
            .unwrap(),
            0,
        )
        .unwrap();
    let owner = AuthoringPropertyOwner::Item(item_id);
    service
        .set_authored_property_constant(owner, "opacity".to_string(), PropertyValue::from(0.25))
        .unwrap();
    let (stale_project, stale_revision) = service.snapshot_with_revision().unwrap();
    service
        .set_authored_property_constant(owner, "opacity".to_string(), PropertyValue::from(0.75))
        .unwrap();

    let mut state = AuthoringUiState::new(stale_project.root_timeline_id);
    let selection = Some(AuthoringSelection::Item(item_id));
    assert!(!sync_draft(
        &stale_project,
        &mut state,
        selection,
        stale_revision
    ));
    let stale_property = stale_project.items[&item_id]
        .authored_properties
        .get("opacity")
        .unwrap();
    let edit = property_authoring::authored_transient_edit(
        state.inspector.synced_revision,
        owner,
        "opacity",
        Some(stale_property),
        MediaTime::zero(),
        PropertyValue::from(0.5),
    )
    .unwrap();
    let projected = edit.project(&stale_project).unwrap();
    assert_eq!(
        projected.items[&item_id]
            .authored_properties
            .get("opacity")
            .unwrap()
            .value(),
        Some(&PropertyValue::from(0.5))
    );

    let error = edit.commit(&service).unwrap_err();
    assert!(error.to_string().contains("Project is now at revision"));
    let current = service.snapshot().unwrap();
    assert_eq!(
        current.items[&item_id]
            .authored_properties
            .get("opacity")
            .unwrap()
            .value(),
        Some(&PropertyValue::from(0.75))
    );
}
