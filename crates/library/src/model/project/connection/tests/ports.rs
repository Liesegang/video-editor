use super::*;

#[test]
fn numeric_union_accepts_each_concrete_numeric_type_in_both_directions() {
    for concrete in [
        PortDataType::Integer,
        PortDataType::Number,
        PortDataType::Vec2,
        PortDataType::Vec3,
        PortDataType::Vec4,
    ] {
        assert!(PortDataType::Numeric.accepts(concrete));
        assert!(concrete.accepts(PortDataType::Numeric));
    }
    assert!(!PortDataType::Numeric.accepts(PortDataType::Color));
    assert!(!PortDataType::Image.accepts(PortDataType::Numeric));
}

#[test]
fn canonical_node_port_order_is_stable_and_does_not_mutate_graph_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let mut project = Project::new("port order");
    let (composition, track) = Composition::new("main", 640, 360, 30.0, 5.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let clip = Clip::new("port order", 0.0, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let container = NodeContainer::Clip(clip_id);
    let plugins = PluginManager::default();
    let shape_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "Shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 10 V 10 Z".to_string(),
            },
        ),
    )?;
    let style = plugins.create_style_operation_node("fill")?;
    let NodeContent::PluginOperation(operation) = style.content() else {
        return Err("Fill factory did not produce a PluginOperation".into());
    };
    let persisted_order = operation
        .declared_ports
        .iter()
        .map(|port| port.key.clone())
        .collect::<Vec<_>>();
    let style_id = attach_authored_node(&mut project, container, style)?;

    assert_eq!(
        project
            .port_definitions(PortOwner::Node(style_id))
            .into_iter()
            .map(|port| port.key)
            .collect::<Vec<_>>(),
        vec![
            TIME_PORT,
            SHAPE_INPUT_PORT,
            "property:color",
            "property:opacity",
            "property:offset",
            IMAGE_OUTPUT_PORT,
        ]
    );
    let NodeContent::PluginOperation(operation) = project.get_node(style_id).unwrap().content()
    else {
        return Err("persisted Fill Node changed content kind".into());
    };
    assert_eq!(
        operation
            .declared_ports
            .iter()
            .map(|port| port.key.clone())
            .collect::<Vec<_>>(),
        persisted_order,
        "derived display ordering must not rewrite persisted plugin ports"
    );

    project.connect_ports(
        PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
    )?;
    let validation_errors = project.validate_connections();
    assert!(validation_errors.is_empty(), "{validation_errors:#?}");

    let fmod = Node::new_fmod("Fmod");
    let fmod_id = attach_authored_node(&mut project, container, fmod)?;
    assert_eq!(
        project
            .port_definitions(PortOwner::Node(fmod_id))
            .into_iter()
            .map(|port| port.key)
            .collect::<Vec<_>>(),
        vec![
            FMOD_X_INPUT_PORT,
            FMOD_DIVISOR_INPUT_PORT,
            NUMBER_RESULT_OUTPUT_PORT,
        ],
        "Fmod is generic and must not gain an implicit Time port"
    );

    let add_id = attach_authored_node(&mut project, container, Node::new_add("Add"))?;
    assert_eq!(
        project
            .port_definitions(PortOwner::Node(add_id))
            .into_iter()
            .map(|port| port.key)
            .collect::<Vec<_>>(),
        vec![
            NUMERIC_A_INPUT_PORT,
            NUMERIC_B_INPUT_PORT,
            NUMBER_RESULT_OUTPUT_PORT,
        ]
    );

    let rms_id = attach_authored_node(
        &mut project,
        container,
        Node::new_sound_analysis("RMS", crate::model::SoundAnalysisContent::Rms),
    )?;
    assert_eq!(
        project
            .port_definitions(PortOwner::Node(rms_id))
            .into_iter()
            .map(|port| (port.key, port.data_type))
            .collect::<Vec<_>>(),
        vec![
            (SOUND_INPUT_PORT.to_string(), PortDataType::Audio),
            (
                ANALYSIS_WINDOW_MS_PROPERTY.to_string(),
                PortDataType::Numeric,
            ),
            (ANALYSIS_HOP_MS_PROPERTY.to_string(), PortDataType::Numeric,),
            (
                ANALYSIS_SAMPLE_RATE_PROPERTY.to_string(),
                PortDataType::Numeric,
            ),
            (NUMBER_RESULT_OUTPUT_PORT.to_string(), PortDataType::Number,),
        ],
        "Sound settings are canonical wire-overridable Numeric inputs"
    );
    Ok(())
}
