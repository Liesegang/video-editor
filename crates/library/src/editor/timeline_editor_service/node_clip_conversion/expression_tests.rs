use super::*;

#[test]
fn expression_source_property_refuses_conversion_without_mutation() {
    let plugins = PluginManager::default();
    let project = AuthoringProject::new(
        "Atomic expression refusal",
        96,
        64,
        RationalRate::new(30, 1).unwrap(),
        MediaTime::from_whole_seconds(2),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let service = TimelineEditorService::new(project).unwrap();
    let (shape_id, _) = service
        .add_item(
            track_id,
            "Animated Rectangle".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::new(),
                    appearance_operations: Vec::new(),
                },
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::from_whole_seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    service
        .set_authored_property(
            AuthoringPropertyOwner::Item(shape_id),
            "width".to_string(),
            Property::expression(
                "100.0 + time * 25.0".to_string(),
                PropertyValue::from(100.0),
            ),
        )
        .unwrap();
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    let error = service
        .convert_source_to_node_clip(&plugins, shape_id)
        .unwrap_err();

    assert!(error.to_string().contains("evaluator 'expression'"));
    assert_eq!(service.revision().unwrap(), revision);
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}
