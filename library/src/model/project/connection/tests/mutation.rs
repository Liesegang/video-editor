use super::*;

#[test]
fn reconnect_reorder_and_splice_keep_the_downstream_wire_identity_and_blend() {
    let mut project = Project::new("connection editing");
    let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let container = NodeContainer::Composition(composition_id);
    let source = add_node(&mut project, container, "source");
    let alternate_source = add_node(&mut project, container, "alternate source");
    let sibling = add_node(&mut project, container, "sibling");
    let via = add_node(&mut project, container, "via");
    let target = add_node(&mut project, container, "target");

    let target_address = PortAddress::new(PortOwner::Node(target), MERGE_IMAGES_PORT);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(sibling), IMAGE_OUTPUT_PORT),
            target_address.clone(),
        )
        .unwrap();
    let connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT),
            target_address.clone(),
        )
        .unwrap();
    project
        .set_connection_blend_mode(connection_id, BlendMode::Multiply)
        .unwrap();
    project.reorder_connection(connection_id, 0).unwrap();
    let reordered = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .unwrap();
    assert_eq!(
        reordered.from,
        PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT)
    );
    assert_eq!(reordered.to, target_address);
    assert_eq!(reordered.blend_mode, BlendMode::Multiply);
    let original_order = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .unwrap()
        .order;

    project
        .reconnect_connection(
            connection_id,
            PortAddress::new(PortOwner::Node(alternate_source), IMAGE_OUTPUT_PORT),
            target_address.clone(),
        )
        .unwrap();
    let reconnected = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .unwrap();
    assert_eq!(reconnected.to, target_address);
    assert_eq!(reconnected.order, original_order);
    assert_eq!(reconnected.blend_mode, BlendMode::Multiply);

    let upstream_id = project
        .splice_connection(
            connection_id,
            PortAddress::new(PortOwner::Node(via), MERGE_IMAGES_PORT),
            PortAddress::new(PortOwner::Node(via), IMAGE_OUTPUT_PORT),
        )
        .unwrap();
    let downstream = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .unwrap();
    assert_eq!(downstream.from.owner, PortOwner::Node(via));
    assert_eq!(downstream.to, target_address);
    assert_eq!(downstream.order, original_order);
    assert_eq!(downstream.blend_mode, BlendMode::Multiply);
    let upstream = project
        .connections
        .iter()
        .find(|connection| connection.id == upstream_id)
        .unwrap();
    assert_eq!(upstream.from.owner, PortOwner::Node(alternate_source));
    assert_eq!(upstream.to.owner, PortOwner::Node(via));
    assert_eq!(upstream.blend_mode, BlendMode::Normal);
    assert!(project.validate_connections().is_empty());
}

#[test]
fn blend_modes_are_fanout_specific_and_invalid_assignments_are_atomic() {
    let mut project = Project::new("wire blend contracts");
    let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let container = NodeContainer::Composition(composition_id);
    let source = add_node(&mut project, container, "source");
    let first_merge = add_node(&mut project, container, "first merge");
    let second_merge = add_node(&mut project, container, "second merge");
    let reference = add_single_image_node(&mut project, container, "single image");

    let source_output = PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT);
    let first_wire = project
        .connect_ports(
            source_output.clone(),
            PortAddress::new(PortOwner::Node(first_merge), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let second_wire = project
        .connect_ports(
            source_output.clone(),
            PortAddress::new(PortOwner::Node(second_merge), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_connection_blend_mode(first_wire, BlendMode::LinearDodge)
        .unwrap();
    project
        .set_connection_blend_mode(second_wire, BlendMode::Multiply)
        .unwrap();
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == first_wire)
            .unwrap()
            .blend_mode,
        BlendMode::LinearDodge
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == second_wire)
            .unwrap()
            .blend_mode,
        BlendMode::Multiply
    );

    let non_merge_target = PortAddress::new(PortOwner::Node(reference), IMAGE_INPUT_PORT);
    let non_merge_wire = project
        .connect_ports(source_output, non_merge_target.clone())
        .unwrap();
    let before_non_merge = project.clone();
    let before_non_merge_bytes = project.save().unwrap();
    assert_eq!(
        project
            .set_connection_blend_mode(non_merge_wire, BlendMode::Screen)
            .unwrap_err(),
        ProjectGraphError::ConnectionBlendRequiresMergeImagesInput {
            connection_id: non_merge_wire,
            blend_mode: BlendMode::Screen,
            target: non_merge_target.clone(),
        }
    );
    assert_eq!(project, before_non_merge);
    assert_eq!(project.save().unwrap(), before_non_merge_bytes);

    let number_wire = project
        .connect_ports(
            PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(source), TIME_PORT),
        )
        .unwrap();
    let before_number = project.clone();
    let before_number_bytes = project.save().unwrap();
    assert_eq!(
        project
            .set_connection_blend_mode(number_wire, BlendMode::Overlay)
            .unwrap_err(),
        ProjectGraphError::ConnectionBlendRequiresImageSource {
            connection_id: number_wire,
            blend_mode: BlendMode::Overlay,
        }
    );
    assert_eq!(project, before_number);
    assert_eq!(project.save().unwrap(), before_number_bytes);

    project
        .connections
        .iter_mut()
        .find(|connection| connection.id == non_merge_wire)
        .unwrap()
        .blend_mode = BlendMode::Screen;
    project
        .connections
        .iter_mut()
        .find(|connection| connection.id == number_wire)
        .unwrap()
        .blend_mode = BlendMode::Overlay;
    let malformed_bytes = project.save().unwrap();
    assert!(matches!(
        project
            .set_connection_blend_mode(non_merge_wire, BlendMode::Screen)
            .unwrap_err(),
        ProjectGraphError::ConnectionBlendRequiresMergeImagesInput { .. }
    ));
    assert_eq!(project.save().unwrap(), malformed_bytes);
    let errors = project.validate_connections();
    assert!(errors.contains(
        &ProjectGraphError::ConnectionBlendRequiresMergeImagesInput {
            connection_id: non_merge_wire,
            blend_mode: BlendMode::Screen,
            target: non_merge_target,
        }
    ));
    assert!(
        errors.contains(&ProjectGraphError::ConnectionBlendRequiresImageSource {
            connection_id: number_wire,
            blend_mode: BlendMode::Overlay,
        })
    );
}

#[test]
fn reconnect_is_atomic_replaces_single_inputs_and_normalizes_variadic_orders() {
    let mut project = Project::new("reconnect contracts");
    let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let container = NodeContainer::Composition(composition_id);
    let sources = (0..5)
        .map(|index| add_node(&mut project, container, &format!("source {index}")))
        .collect::<Vec<_>>();
    let target_a = add_node(&mut project, container, "target a");
    let target_b = add_node(&mut project, container, "target b");
    let single_a = add_single_image_node(&mut project, container, "single a");
    let single_b = add_single_image_node(&mut project, container, "single b");

    let single_a_input = PortAddress::new(PortOwner::Node(single_a), IMAGE_INPUT_PORT);
    let occupied_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(sources[0]), IMAGE_OUTPUT_PORT),
            single_a_input.clone(),
        )
        .unwrap();
    let moving_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(sources[1]), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(single_b), IMAGE_INPUT_PORT),
        )
        .unwrap();
    project
        .reconnect_connection(
            moving_id,
            PortAddress::new(PortOwner::Node(sources[1]), IMAGE_OUTPUT_PORT),
            single_a_input.clone(),
        )
        .unwrap();
    assert!(
        !project
            .connections
            .iter()
            .any(|item| item.id == occupied_id)
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|item| item.id == moving_id)
            .map(|item| (&item.to, item.order)),
        Some((&single_a_input, 0))
    );

    let target_a_input = PortAddress::new(PortOwner::Node(target_a), MERGE_IMAGES_PORT);
    let target_b_input = PortAddress::new(PortOwner::Node(target_b), MERGE_IMAGES_PORT);
    for source in &sources[..3] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(*source), IMAGE_OUTPUT_PORT),
                target_a_input.clone(),
            )
            .unwrap();
    }
    let target_b_existing = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(sources[3]), IMAGE_OUTPUT_PORT),
            target_b_input.clone(),
        )
        .unwrap();
    project
        .set_connection_blend_mode(target_b_existing, BlendMode::Screen)
        .unwrap();
    let moved_variadic = project
        .connections
        .iter()
        .find(|item| item.from.owner == PortOwner::Node(sources[2]) && item.to == target_a_input)
        .unwrap()
        .id;
    project
        .set_connection_blend_mode(moved_variadic, BlendMode::LinearDodge)
        .unwrap();
    project
        .reconnect_connection(
            moved_variadic,
            PortAddress::new(PortOwner::Node(sources[2]), IMAGE_OUTPUT_PORT),
            target_b_input.clone(),
        )
        .unwrap();
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == moved_variadic)
            .unwrap()
            .blend_mode,
        BlendMode::LinearDodge,
    );
    let orders = |project: &Project, target: &PortAddress| {
        let mut orders = project
            .connections
            .iter()
            .filter(|item| &item.to == target)
            .map(|item| item.order)
            .collect::<Vec<_>>();
        orders.sort_unstable();
        orders
    };
    assert_eq!(orders(&project, &target_a_input), vec![0, 1]);
    assert_eq!(orders(&project, &target_b_input), vec![0, 1]);

    let before_invalid = project.clone();
    let error = project
        .reconnect_connection(
            moved_variadic,
            PortAddress::new(PortOwner::Node(sources[2]), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(target_b), TIME_PORT),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ProjectGraphError::ConnectionBlendRequiresMergeImagesInput {
            connection_id,
            blend_mode: BlendMode::LinearDodge,
            ..
        } if connection_id == moved_variadic
    ));
    assert_eq!(project, before_invalid, "failed reconnect must roll back");

    for (order, connection) in project
        .connections
        .iter_mut()
        .filter(|connection| connection.to == target_b_input)
        .enumerate()
    {
        connection.order = 4 + order as i64 * 5;
    }
    let unaffected_before = project
        .connections
        .iter()
        .filter(|connection| connection.to == target_b_input)
        .cloned()
        .collect::<Vec<_>>();
    let first_target_a = project
        .connections
        .iter()
        .find(|connection| connection.to == target_a_input && connection.order == 0)
        .unwrap()
        .id;
    assert_eq!(project.disconnect_connections([first_target_a]), 1);
    assert_eq!(orders(&project, &target_a_input), vec![0]);
    assert_eq!(
        project
            .connections
            .iter()
            .filter(|connection| connection.to == target_b_input)
            .cloned()
            .collect::<Vec<_>>(),
        unaffected_before,
        "unaffected wires must be byte-for-byte stable",
    );

    let remaining_a = project
        .connections
        .iter()
        .find(|connection| connection.to == target_a_input)
        .unwrap()
        .id;
    let first_b = unaffected_before[0].id;
    let surviving_b_blend = unaffected_before[1].blend_mode;
    assert_eq!(project.disconnect_connections([remaining_a, first_b]), 2);
    assert!(orders(&project, &target_a_input).is_empty());
    assert_eq!(orders(&project, &target_b_input), vec![0]);
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.to == target_b_input)
            .unwrap()
            .blend_mode,
        surviving_b_blend,
    );
}

#[test]
fn reconnect_allows_cross_container_graph_ports_but_rejects_internal_escape_and_cycles() {
    let mut project = Project::new("reconnect scope contracts");
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

    let composition_source = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        "composition source",
    );
    let clip_source = add_node(&mut project, NodeContainer::Clip(clip_id), "clip source");
    let clip_target = add_node(&mut project, NodeContainer::Clip(clip_id), "clip target");
    let image_connection = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(clip_source), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(clip_target), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .reconnect_connection(
            image_connection,
            PortAddress::new(PortOwner::Node(composition_source), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(clip_target), MERGE_IMAGES_PORT),
        )
        .unwrap();

    let time_connection = project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(clip_source), TIME_PORT),
        )
        .unwrap();
    let before_escape = project.clone();
    let error = project
        .reconnect_connection(
            time_connection,
            PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(clip_source), TIME_PORT),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ProjectGraphError::InternalPortEscapesContainer { .. }
    ));
    assert_eq!(project, before_escape);

    let a = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        "a",
    );
    let b = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        "b",
    );
    let c = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        "c",
    );
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(a), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(b), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let movable = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(c), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(clip_target), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let before_cycle = project.clone();
    let error = project
        .reconnect_connection(
            movable,
            PortAddress::new(PortOwner::Node(b), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(a), MERGE_IMAGES_PORT),
        )
        .unwrap_err();
    assert!(matches!(error, ProjectGraphError::ConnectionCycle { .. }));
    assert_eq!(project, before_cycle);
}

#[test]
fn splice_rejects_occupied_single_input_and_any_validation_failure_without_mutation() {
    let mut project = Project::new("splice rollback");
    let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let container = NodeContainer::Composition(composition_id);
    let source = add_node(&mut project, container, "source");
    let occupant = add_node(&mut project, container, "occupant");
    let via = add_single_image_node(&mut project, container, "via");
    let target = add_node(&mut project, container, "target");
    let connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(target), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let via_input = PortAddress::new(PortOwner::Node(via), IMAGE_INPUT_PORT);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(occupant), IMAGE_OUTPUT_PORT),
            via_input.clone(),
        )
        .unwrap();
    let before_occupied = project.clone();
    assert_eq!(
        project
            .splice_connection(
                connection_id,
                via_input.clone(),
                PortAddress::new(PortOwner::Node(via), IMAGE_OUTPUT_PORT),
            )
            .unwrap_err(),
        ProjectGraphError::SpliceInputOccupied { target: via_input }
    );
    assert_eq!(project, before_occupied);

    let empty_via = add_single_image_node(&mut project, container, "empty via");
    let before_invalid = project.clone();
    let error = project
        .splice_connection(
            connection_id,
            PortAddress::new(PortOwner::Node(empty_via), IMAGE_INPUT_PORT),
            PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ProjectGraphError::IncompatiblePortTypes { .. }
            | ProjectGraphError::InternalPortEscapesContainer { .. }
    ));
    assert_eq!(project, before_invalid);
}
