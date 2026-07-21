use super::*;

#[test]
fn container_graph_semantics_follow_the_complete_shape_to_image_chain()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, clip_id) = project_with_detached_clip("title", 0.0, 5.0);
    let container = NodeContainer::Clip(clip_id);
    let plugins = PluginManager::default();
    let text_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "Title",
            GeneratorNodeRequest::Text {
                text: "Title".to_string(),
                font: "Arial".to_string(),
            },
        ),
    )?;
    let decorator = plugins.create_decorator_operation_node("backplate")?;
    let decorator_id = attach_authored_node(&mut project, container, decorator)?;
    let effector = plugins.create_effector_operation_node("transform")?;
    let effector_id = attach_authored_node(&mut project, container, effector)?;
    let style = plugins.create_style_operation_node("fill")?;
    let style_id = attach_authored_node(&mut project, container, style)?;
    let effect = plugins.create_effect_operation_node("blur")?;
    let effect_id = attach_authored_node(&mut project, container, effect)?;
    let merge_id = attach_authored_node(&mut project, container, Node::new_merge("Result"))?;

    for (from, from_port, to, to_port) in [
        (text_id, SHAPE_OUTPUT_PORT, decorator_id, SHAPE_INPUT_PORT),
        (
            decorator_id,
            SHAPE_OUTPUT_PORT,
            effector_id,
            SHAPE_INPUT_PORT,
        ),
        (effector_id, SHAPE_OUTPUT_PORT, style_id, SHAPE_INPUT_PORT),
        (style_id, IMAGE_OUTPUT_PORT, effect_id, IMAGE_INPUT_PORT),
        (effect_id, IMAGE_OUTPUT_PORT, merge_id, MERGE_IMAGES_PORT),
    ] {
        project.connect_ports(
            PortAddress::new(PortOwner::Node(from), from_port),
            PortAddress::new(PortOwner::Node(to), to_port),
        )?;
    }
    project.set_output_node(container, Some(merge_id))?;

    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(semantics.explicit_output_node_id(), Some(merge_id));
    assert_eq!(semantics.authored_source(), Some(PortOwner::Node(text_id)));
    for node_id in [
        text_id,
        decorator_id,
        effector_id,
        style_id,
        effect_id,
        merge_id,
    ] {
        assert!(semantics.structurally_reaches_output(PortOwner::Node(node_id)));
    }
    Ok(())
}

#[test]
fn container_graph_semantics_include_every_reachable_fan_out_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, clip_id) = project_with_detached_clip("shape", 0.0, 5.0);
    let container = NodeContainer::Clip(clip_id);
    let plugins = PluginManager::default();
    let shape_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "Shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        ),
    )?;
    let fill = plugins.create_style_operation_node("fill")?;
    let fill_id = attach_authored_node(&mut project, container, fill)?;
    let stroke = plugins.create_style_operation_node("stroke")?;
    let stroke_id = attach_authored_node(&mut project, container, stroke)?;
    let merge_id = attach_authored_node(&mut project, container, Node::new_merge("Result"))?;

    for style_id in [fill_id, stroke_id] {
        project.connect_ports(
            PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
    }
    project.set_output_node(container, Some(merge_id))?;

    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(semantics.authored_source(), Some(PortOwner::Node(shape_id)));
    for node_id in [shape_id, fill_id, stroke_id, merge_id] {
        assert!(semantics.structurally_reaches_output(PortOwner::Node(node_id)));
    }
    Ok(())
}

#[test]
fn explicit_output_binding_selects_identity_instead_of_storage_order()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, clip_id) = project_with_detached_clip("two sources", 0.0, 5.0);
    let container = NodeContainer::Clip(clip_id);
    let first_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "First",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;
    let second_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "Second",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;

    project.set_output_node(container, Some(second_id))?;
    let second = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(second.explicit_output_node_id(), Some(second_id));
    assert!(second.explicit_output_is_directly_contained());
    assert_eq!(second.authored_source_node_id(), Some(second_id));
    assert!(!second.structurally_reaches_output(PortOwner::Node(first_id)));

    project.set_output_node(container, Some(first_id))?;
    let first = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(first.explicit_output_node_id(), Some(first_id));
    assert_eq!(first.authored_source_node_id(), Some(first_id));
    assert!(!first.structurally_reaches_output(PortOwner::Node(second_id)));
    Ok(())
}

#[test]
fn foreign_output_binding_remains_observable_without_crossing_container_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, first_clip_id) = project_with_detached_clip("first clip", 0.0, 5.0);
    let second_clip = Clip::new("second clip", 0.0, 5.0);
    let second_clip_id = second_clip.id;
    project.add_clip(second_clip);
    let foreign_source_id = attach_authored_node(
        &mut project,
        NodeContainer::Clip(second_clip_id),
        test_generator_node(
            "foreign source",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;
    project.set_output_node(NodeContainer::Clip(second_clip_id), Some(foreign_source_id))?;

    // Normal mutations reject this cross-owner binding. Retain the raw
    // authored UUID while proving the read-only facade cannot escape its
    // requested container in a malformed, directly loaded Project.
    project
        .get_clip_mut(first_clip_id)
        .ok_or(ProjectGraphError::ClipNotFound(first_clip_id))?
        .output_node_id = Some(foreign_source_id);

    let semantics = project.container_graph_semantics(PortOwner::Clip(first_clip_id));
    assert_eq!(semantics.explicit_output_node_id(), Some(foreign_source_id));
    assert!(!semantics.explicit_output_is_directly_contained());
    assert_eq!(semantics.authored_source(), None);
    assert!(!semantics.structurally_reaches_output(PortOwner::Node(foreign_source_id)));
    Ok(())
}

#[test]
fn composition_instance_is_an_authored_identity_terminal_without_an_image_override()
-> Result<(), Box<dyn std::error::Error>> {
    let mut project = Project::new("composition instance identity");
    let (target, target_track) = Composition::new("target", 640, 360, 30.0, 5.0);
    let target_id = target.id;
    project.add_track(target_track)?;
    project.add_composition(target)?;
    let (parent, parent_track) = Composition::new("parent", 640, 360, 30.0, 5.0);
    let parent_track_id = parent_track.id;
    project.add_track(parent_track)?;
    project.add_composition(parent)?;
    let clip = Clip::new("instance", 0.0, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(parent_track_id, clip_id)?;
    let container = NodeContainer::Clip(clip_id);
    let instance_id = attach_authored_node(
        &mut project,
        container,
        Node::new_composition_instance(
            "instance",
            CompositionInstanceContent {
                composition_id: target_id,
            },
        ),
    )?;
    project.set_output_node(container, Some(instance_id))?;

    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(semantics.authored_source_node_id(), Some(instance_id));
    assert!(semantics.structurally_reaches_output(PortOwner::Node(instance_id)));
    Ok(())
}

#[test]
fn authored_identity_ignores_disabled_state_and_clip_time_range()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, clip_id) = project_with_detached_clip("late clip", 100.0, 0.25);
    let container = NodeContainer::Clip(clip_id);
    let source_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "Late source",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;
    let effect = PluginManager::default().create_effect_operation_node("blur")?;
    let effect_id = attach_authored_node(&mut project, container, effect)?;
    project.connect_ports(
        PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
    )?;
    project.set_output_node(container, Some(effect_id))?;

    project
        .get_node_mut(source_id)
        .ok_or(ProjectGraphError::NodeNotFound(source_id))?
        .enabled = false;
    project
        .get_node_mut(effect_id)
        .ok_or(ProjectGraphError::NodeNotFound(effect_id))?
        .enabled = false;
    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(semantics.explicit_output_node_id(), Some(effect_id));
    assert_eq!(semantics.authored_source_node_id(), Some(source_id));
    assert!(semantics.structurally_reaches_output(PortOwner::Node(source_id)));
    Ok(())
}

#[test]
fn direct_track_and_composition_nodes_follow_cross_container_image_wires()
-> Result<(), Box<dyn std::error::Error>> {
    let mut project = Project::new("direct container nodes");
    let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let plugins = PluginManager::default();

    let track_source_id = attach_authored_node(
        &mut project,
        NodeContainer::Track(track_id),
        test_generator_node(
            "Track source",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;
    let track_effect = plugins.create_effect_operation_node("blur")?;
    let track_effect_id =
        attach_authored_node(&mut project, NodeContainer::Track(track_id), track_effect)?;
    let track_merge_id = project
        .get_track(track_id)
        .ok_or(ProjectGraphError::TrackNotFound(track_id))?
        .structural_merge_node_id;
    project.connect_ports(
        PortAddress::new(PortOwner::Node(track_source_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(track_merge_id), MERGE_IMAGES_PORT),
    )?;
    project.connect_ports(
        PortAddress::new(PortOwner::Node(track_merge_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(track_effect_id), IMAGE_INPUT_PORT),
    )?;
    project.set_output_node(NodeContainer::Track(track_id), Some(track_effect_id))?;

    let composition_effect = plugins.create_effect_operation_node("blur")?;
    let composition_effect_id = attach_authored_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        composition_effect,
    )?;
    let composition_merge_id = project
        .get_composition(composition_id)
        .ok_or(ProjectGraphError::CompositionNotFound(composition_id))?
        .structural_merge_node_id;
    project.connect_ports(
        PortAddress::new(PortOwner::Node(composition_merge_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(composition_effect_id), IMAGE_INPUT_PORT),
    )?;
    project.set_output_node(
        NodeContainer::Composition(composition_id),
        Some(composition_effect_id),
    )?;

    let track = project.container_graph_semantics(PortOwner::Track(track_id));
    assert_eq!(track.explicit_output_node_id(), Some(track_effect_id));
    assert_eq!(track.authored_source_node_id(), Some(track_source_id));

    let composition = project.container_graph_semantics(PortOwner::Composition(composition_id));
    assert_eq!(
        composition.explicit_output_node_id(),
        Some(composition_effect_id)
    );
    assert_eq!(composition.authored_source_node_id(), Some(track_source_id));
    for owner in [
        PortOwner::Node(composition_effect_id),
        PortOwner::Track(track_id),
        PortOwner::Node(track_effect_id),
        PortOwner::Node(track_source_id),
    ] {
        assert!(composition.structurally_reaches_output(owner));
    }
    Ok(())
}

#[test]
fn dead_cycle_and_missing_owner_do_not_poison_a_later_authored_source()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, clip_id) = project_with_detached_clip("damaged branches", 0.0, 5.0);
    let container = NodeContainer::Clip(clip_id);
    let cycle_a = attach_authored_node(&mut project, container, Node::new_merge("cycle a"))?;
    let cycle_b = attach_authored_node(&mut project, container, Node::new_merge("cycle b"))?;
    let valid_source = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "valid source",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;
    let result = attach_authored_node(&mut project, container, Node::new_merge("result"))?;
    project.set_output_node(container, Some(result))?;

    // Normal mutations reject this state. Insert it directly to prove the
    // read-only query remains finite and continues to a valid later input.
    project.connections.extend([
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(cycle_b), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(cycle_a), MERGE_IMAGES_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(cycle_a), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(cycle_b), MERGE_IMAGES_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(cycle_a), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(result), MERGE_IMAGES_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(result), MERGE_IMAGES_PORT),
            1,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(valid_source), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(result), MERGE_IMAGES_PORT),
            2,
        ),
    ]);

    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(semantics.authored_source_node_id(), Some(valid_source));
    assert!(semantics.structurally_reaches_output(PortOwner::Node(valid_source)));
    Ok(())
}

#[test]
fn container_graph_semantics_scale_deterministically_over_a_long_visual_chain()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut project, clip_id) = project_with_detached_clip("long chain", 0.0, 5.0);
    let container = NodeContainer::Clip(clip_id);
    let source_id = attach_authored_node(
        &mut project,
        container,
        test_generator_node(
            "source",
            GeneratorNodeRequest::Solid {
                color: crate::model::frame::color::Color::black(),
            },
        ),
    )?;
    let mut previous_id = source_id;
    let mut chain_ids = Vec::new();
    let mut connections = Vec::new();
    for index in 0..256 {
        let merge_id = attach_authored_node(
            &mut project,
            container,
            Node::new_merge(&format!("merge {index}")),
        )?;
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(previous_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            0,
        ));
        chain_ids.push(merge_id);
        previous_id = merge_id;
    }
    project.set_output_node(container, Some(previous_id))?;
    project.connections.extend(connections);

    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    assert_eq!(semantics.authored_source_node_id(), Some(source_id));
    assert!(semantics.structurally_reaches_output(PortOwner::Node(source_id)));
    for node_id in chain_ids {
        assert!(semantics.structurally_reaches_output(PortOwner::Node(node_id)));
    }
    assert_eq!(
        semantics,
        project.container_graph_semantics(PortOwner::Clip(clip_id))
    );
    Ok(())
}

#[test]
fn container_image_sources_use_only_structural_merge_graphs_and_output_bindings() {
    let mut project = Project::new("container sources");
    let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let clip = Clip::new("clip", 0.0, 10.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let composition_merge_id = project
        .get_composition(composition_id)
        .unwrap()
        .structural_merge_node_id;
    let track_merge_id = project
        .get_track(track_id)
        .unwrap()
        .structural_merge_node_id;
    let second_clip_node = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        "second clip node",
    );
    assert_eq!(
        project.container_image_sources(PortOwner::Composition(composition_id)),
        vec![ContainerImageSource {
            source: PortOwner::Node(composition_merge_id),
            kind: ContainerImageSourceKind::OutputBinding,
        }]
    );
    assert_eq!(
        project.container_image_sources(PortOwner::Track(track_id)),
        vec![ContainerImageSource {
            source: PortOwner::Node(track_merge_id),
            kind: ContainerImageSourceKind::OutputBinding,
        }]
    );
    assert!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .is_empty(),
        "Clip nodes are internal graph values until an output is bound"
    );

    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(track_merge_id), MERGE_IMAGES_PORT)
    }));
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Track(track_id), IMAGE_OUTPUT_PORT)
            && connection.to
                == PortAddress::new(PortOwner::Node(composition_merge_id), MERGE_IMAGES_PORT)
    }));

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(second_clip_node))
        .unwrap();
    let downstream_track_merge = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        "downstream track merge",
    );
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(track_merge_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(downstream_track_merge), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Track(track_id), Some(downstream_track_merge))
        .unwrap();
    assert_eq!(
        project.container_image_sources(PortOwner::Track(track_id)),
        vec![ContainerImageSource {
            source: PortOwner::Node(downstream_track_merge),
            kind: ContainerImageSourceKind::OutputBinding,
        }]
    );

    assert_eq!(
        project.container_image_sources(PortOwner::Clip(clip_id)),
        vec![ContainerImageSource {
            source: PortOwner::Node(second_clip_node),
            kind: ContainerImageSourceKind::OutputBinding,
        }]
    );
    let unrelated = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        "unrelated track merge",
    );
    let before = project.clone();
    assert!(matches!(
        project.set_output_node(NodeContainer::Track(track_id), Some(unrelated)),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput { .. })
    ));
    assert_eq!(project, before);
    assert!(
        project
            .container_image_sources(PortOwner::Node(second_clip_node))
            .is_empty()
    );
    assert!(
        project
            .container_image_sources(PortOwner::Track(Uuid::new_v4()))
            .is_empty()
    );
}
