use super::*;
use library::model::authoring::{
    CompositionInstance, DurationPolicy, InstancePath, MediaTime, ModuleTemplateOrigin,
    RationalRate, TimelineInterval, TimelineItemId, TransitionId,
};

#[test]
fn transition_breadcrumb_identifies_media_sharing_and_timeline_scope() {
    for (media_type, expected_label, expected_media) in [
        (TransitionMediaType::Image, "Image Transition", "image"),
        (TransitionMediaType::Audio, "Audio Transition", "audio"),
    ] {
        let (definition, _) = ModuleDefinition::new_transition(
            "Reusable Transition",
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
            media_type,
        )
        .expect("Transition Module fixture");
        let host = ModuleEditorHost::Transition {
            transition_id: TransitionId::new(),
            instance_path: None,
            module_instance_id: ModuleInstanceId::new(),
        };
        let presentation = module_host_presentation(&definition, &host);

        assert_eq!(presentation.label, expected_label);
        assert_eq!(host.kind_name(), "transition");
        assert_eq!(presentation.media_type, Some(expected_media));
        let sharing = module_sharing_presentation(&definition.sharing, true);
        assert_eq!(sharing.label, "Reusable");
        assert!(!sharing.tooltip.contains("this Module instance"));
        assert!(sharing.tooltip.contains("all of its concrete placements"));
    }

    let private = module_sharing_presentation(&ModuleDefinitionSharing::Private, true);
    assert_eq!(private.label, "Private");
    assert!(!private.tooltip.contains("this Module instance"));
    assert!(private.tooltip.contains("every concrete placement"));
}

#[test]
fn transition_breadcrumb_metadata_keeps_path_and_definition_impact() {
    let (definition, _) = ModuleDefinition::new_transition(
        "Transition",
        ModuleDefinitionSharing::Private,
        TransitionMediaType::Image,
    )
    .expect("Transition Module fixture");
    let transition_id = TransitionId::new();
    let timeline_id = TimelineId::new();
    let instance_path = InstancePath::root(TimelineId::new()).nested(TimelineItemId::new());
    let host = ModuleEditorHost::Transition {
        transition_id,
        instance_path: Some(instance_path.clone()),
        module_instance_id: ModuleInstanceId::new(),
    };
    let host_presentation = module_host_presentation(&definition, &host);
    let sharing = module_sharing_presentation(&definition.sharing, true);
    let metadata = document_breadcrumb_metadata(
        &definition,
        &host,
        host_presentation,
        sharing,
        Some(TransitionDefinitionScope {
            transition_id,
            timeline_id,
            timeline_name: "Nested",
            affected_placement_count: 2,
        }),
    );

    assert_eq!(metadata["edit_scope"], "timeline_definition");
    assert_eq!(metadata["instance_edit"], false);
    assert_eq!(metadata["transition_id"], serde_json::json!(transition_id));
    assert_eq!(
        metadata["captured_instance_path"],
        serde_json::json!(instance_path)
    );
    assert_eq!(metadata["timeline_id"], serde_json::json!(timeline_id));
    assert_eq!(metadata["affected_placement_count"], 2);
}

#[test]
fn timeline_definition_placement_count_includes_repeated_nested_placements() {
    let fps = RationalRate::new(30, 1).expect("fixture frame rate");
    let duration = MediaTime::from_whole_seconds(10);
    let project =
        AuthoringProject::new("placement count", 320, 180, fps, duration).expect("fixture Project");
    let service = TimelineEditorService::new(project).expect("fixture service");
    let root = service.snapshot().expect("root snapshot");
    let root_timeline_id = root.root_timeline_id;
    let root_track_id = root.timelines[&root_timeline_id].track_order[0];
    let (nested_timeline_id, _, _) = service
        .add_timeline("Nested".to_string(), 320, 180, fps, duration)
        .expect("nested Timeline");
    for layer in 0..2 {
        service
            .add_item(
                root_track_id,
                format!("Nested {layer}"),
                SourceRef::Composition(CompositionInstance {
                    timeline_id: nested_timeline_id,
                    duration_policy: DurationPolicy::Fixed,
                    parameter_overrides: std::collections::HashMap::new(),
                    transition_module_overrides: Vec::new(),
                }),
                TimelineInterval::new(MediaTime::zero(), duration).expect("placement interval"),
                layer,
            )
            .expect("nested placement");
    }
    let project = service.snapshot().expect("placement snapshot");

    assert_eq!(
        timeline_definition_placement_count(&project, root_timeline_id),
        1
    );
    assert_eq!(
        timeline_definition_placement_count(&project, nested_timeline_id),
        2
    );
}

#[test]
fn plugin_request_keeps_its_operation_identity() {
    let request = ModuleNodeCreateRequest::PluginOperation {
        category: "effect".to_string(),
        component_id: "blur".to_string(),
        operation: "effect.apply.v1".to_string(),
    };
    assert!(matches!(
        authoring_node_request(request),
        Some(ModuleNodeRequest::PluginOperation { category, component_id, operation })
            if category == "effect"
                && component_id == "blur"
                && operation == "effect.apply.v1"
    ));
}

#[test]
fn generator_catalog_entries_use_the_project_independent_factory_requests() {
    assert!(matches!(
        authoring_node_request(ModuleNodeCreateRequest::Native("native.text".to_string())),
        Some(ModuleNodeRequest::Text { .. })
    ));
    assert!(matches!(
        authoring_node_request(ModuleNodeCreateRequest::Native(
            "native.solid-color".to_string()
        )),
        Some(ModuleNodeRequest::Solid { .. })
    ));
    assert!(matches!(
        authoring_node_request(ModuleNodeCreateRequest::Native("native.shape".to_string())),
        Some(ModuleNodeRequest::Shape { .. })
    ));
    assert!(matches!(
        authoring_node_request(ModuleNodeCreateRequest::Native(
            "native.sksl-shader".to_string()
        )),
        Some(ModuleNodeRequest::SkSL { .. })
    ));
}
