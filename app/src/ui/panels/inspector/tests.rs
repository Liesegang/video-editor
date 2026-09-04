use super::*;

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
    state.inspector.text = "text edit in progress".to_string();
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

    assert!(state.inspector.property_values.is_empty());
    assert!(state.inspector.effect_values.is_empty());
    assert_eq!(state.inspector.name, "name edit in progress");
    assert_eq!(state.inspector.text, "text edit in progress");
    assert_eq!(
        state.inspector.expression_sources["item:test:opacity"],
        "value * 2"
    );
    assert_eq!(state.inspector.synced_frame, Some(1));
}
