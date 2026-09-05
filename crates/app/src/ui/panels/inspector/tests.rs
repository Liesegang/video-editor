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
    assert_eq!(state.inspector.synced_frame, Some(1));
}
