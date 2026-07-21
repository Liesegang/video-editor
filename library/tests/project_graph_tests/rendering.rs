use super::*;

#[test]
fn text_and_shape_require_style_before_the_clip_can_output_an_image() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "shape graph")?;
    let text = generator_node_for_canvas(
        "Text",
        GeneratorNodeRequest::Text {
            text: "Text".to_string(),
            font: "Arial".to_string(),
        },
        320,
        180,
        320,
        180,
    );
    let text_id = text.id;
    let shape = generator_node_for_canvas(
        "Shape",
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 100 V 100 Z".to_string(),
        },
        320,
        180,
        100,
        100,
    );
    let shape_id = shape.id;
    let style = plugin_operation_node(
        "Style",
        "style",
        "builtin.style",
        "style.apply",
        vec![
            PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
            graph_output(IMAGE_OUTPUT_PORT, "Image", PortDataType::Image),
        ],
    );
    let style_id = style.id;
    let text_to_style = ProjectConnection::new(
        address(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
        address(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
        0,
    );
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![text, shape, style], vec![text_to_style], None),
    )?;

    for source_id in [text_id, shape_id] {
        let ports = project.port_definitions(PortOwner::Node(source_id));
        assert!(ports.iter().any(|port| {
            port.key == SHAPE_OUTPUT_PORT
                && port.direction == PortDirection::Output
                && port.data_type == PortDataType::Shape
        }));
        assert!(!ports.iter().any(|port| {
            port.key == IMAGE_OUTPUT_PORT && port.direction == PortDirection::Output
        }));
    }
    assert!(
        project
            .port_definition(
                &address(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .is_some_and(|port| port.data_type == PortDataType::Image)
    );
    assert!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .is_empty()
    );
    assert_eq!(
        project.set_output_node(NodeContainer::Clip(clip_id), Some(text_id)),
        Err(ProjectGraphError::OutputNodeHasNoImagePort {
            node_id: text_id,
            container: NodeContainer::Clip(clip_id),
        })
    );

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(style_id))
        .map_err(|error| anyhow!(error))?;
    assert_eq!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(style_id)],
        "only the explicitly bound post-Style Image is the Clip output"
    );
    Ok(())
}

#[test]
fn child_container_images_feeding_a_direct_parent_sink_are_not_double_composed() -> Result<()> {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "child source")?;
    let clip_node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("clip image"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(clip_node_id))
        .map_err(|error| anyhow!(error))?;
    let track_merge_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        Node::new_merge("Track downstream Merge"),
    )?;
    bind_downstream_merge(&mut project, NodeContainer::Track(track_id), track_merge_id)?;
    let composition_merge_id = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        Node::new_merge("Composition downstream Merge"),
    )?;
    bind_downstream_merge(
        &mut project,
        NodeContainer::Composition(composition_id),
        composition_merge_id,
    )?;

    assert_eq!(
        project
            .container_image_sources(PortOwner::Track(track_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(track_merge_id)]
    );
    assert_eq!(
        project
            .container_image_sources(PortOwner::Composition(composition_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(composition_merge_id)]
    );
    assert!(project.validate_connections().is_empty());
    Ok(())
}

#[test]
fn cross_container_image_consumers_do_not_change_the_source_output_binding() -> Result<()> {
    let (mut project, source_composition_id, source_track_id) = project_with_composition();
    let (target_composition, target_track) = Composition::new("target", 320, 180, 30.0, 10.0);
    let target_composition_id = target_composition.id;
    assert!(
        project.add_track(target_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(target_composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let target_merge_id = add_node(
        &mut project,
        NodeContainer::Composition(target_composition_id),
        Node::new_merge("Cross-container Merge"),
    )?;
    project.connect_ports(
        address(PortOwner::Track(source_track_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(target_merge_id), MERGE_IMAGES_PORT),
    )?;

    assert_eq!(
        project
            .container_image_sources(PortOwner::Composition(source_composition_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(structural_merge_id(
            &project,
            NodeContainer::Composition(source_composition_id)
        )?)]
    );
    Ok(())
}

#[test]
fn merge_order_and_wire_blend_change_real_pixels_without_reading_source_blend() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let mut first = colored_solid_node(
        "first",
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
    );
    first.blend_mode = BlendMode::Overlay;
    let first_id = add_node(&mut project, NodeContainer::Clip(clip_id), first)?;
    let mut second = colored_solid_node(
        "second",
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
    );
    second.blend_mode = BlendMode::Screen;
    let second_id = add_node(&mut project, NodeContainer::Clip(clip_id), second)?;
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("merge"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .map_err(|error| anyhow!(error))?;
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let first_connection = project.connect_ports(
        address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
        target.clone(),
    )?;
    let second_connection = project.connect_ports(
        address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
        target,
    )?;
    project.set_connection_blend_mode(second_connection, BlendMode::Multiply)?;

    let rendered = frame(&project, 0)?;
    let merge = find_group(&rendered.items, merge_id).context("Merge group must render")?;
    assert_eq!(merge.kind, FrameGroupKind::Merge);
    assert_eq!(merge.items.len(), 2);
    let wrappers = merge
        .items
        .iter()
        .map(|item| -> Result<(Uuid, BlendMode)> {
            match item {
                FrameItem::Group(group) => Ok((group.source_id, group.blend_mode)),
                FrameItem::Object(_) => bail!("Merge inputs must be isolated present images"),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(
        wrappers,
        vec![
            (first_connection, BlendMode::Normal),
            (second_connection, BlendMode::Multiply),
        ]
    );
    assert_eq!(object_source_ids(&merge.items), vec![first_id, second_id]);
    assert_eq!(
        center_pixel(&preview(&project)?),
        [0, 0, 0, 255],
        "red followed by a green Multiply wire must render black",
    );

    project.reorder_connection(second_connection, 0)?;
    let rendered = frame(&project, 0)?;
    let merge = find_group(&rendered.items, merge_id).context("reordered Merge must render")?;
    let wrappers = merge
        .items
        .iter()
        .map(|item| -> Result<(Uuid, BlendMode)> {
            match item {
                FrameItem::Group(group) => Ok((group.source_id, group.blend_mode)),
                FrameItem::Object(_) => bail!("Merge input wrapper unexpectedly disappeared"),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(
        wrappers,
        vec![
            (second_connection, BlendMode::Normal),
            (first_connection, BlendMode::Normal),
        ]
    );
    assert_eq!(object_source_ids(&merge.items), vec![second_id, first_id]);
    assert_eq!(
        center_pixel(&preview(&project)?),
        [255, 0, 0, 255],
        "the produced green base followed by a Normal red wire must render red",
    );
    Ok(())
}

#[test]
fn composition_instance_materializes_an_empty_target_as_its_opaque_background() -> Result<()> {
    let (mut project, _parent_id, parent_track_id) = project_with_composition();
    let (mut nested, nested_track) = Composition::new("empty nested", 640, 360, 24.0, 2.0);
    let nested_background = Color {
        r: 17,
        g: 34,
        b: 51,
        a: 255,
    };
    nested.background_color = nested_background.clone();
    let nested_id = nested.id;
    assert!(
        project.add_track(nested_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(nested).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let clip_id = add_clip(&mut project, parent_track_id, "composition instance clip")?;
    let instance_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_composition_instance(
            "empty composition instance",
            CompositionInstanceContent {
                composition_id: nested_id,
            },
        ),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(instance_id))
        .map_err(|error| anyhow!(error))?;

    let rendered = frame(&project, 0)?;
    let nested_group =
        find_group(&rendered.items, nested_id).context("nested Composition must render")?;
    assert_eq!(nested_group.kind, FrameGroupKind::Composition);
    assert_eq!((nested_group.width, nested_group.height), (640, 360));
    assert_eq!(nested_group.background_color, nested_background);
    assert!(nested_group.items.is_empty());
    Ok(())
}

#[test]
fn merge_keeps_an_empty_nested_composition_as_a_transparent_produced_input() -> Result<()> {
    let (mut project, parent_id, _parent_track_id) = project_with_composition();
    let (mut nested, nested_track) = Composition::new("transparent nested", 800, 450, 30.0, 2.0);
    let nested_background = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    nested.background_color = nested_background.clone();
    let nested_id = nested.id;
    assert!(
        project.add_track(nested_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(nested).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let merge_id = add_node(
        &mut project,
        NodeContainer::Composition(parent_id),
        Node::new_merge("composition merge"),
    )?;
    bind_downstream_merge(
        &mut project,
        NodeContainer::Composition(parent_id),
        merge_id,
    )?;
    let connection_id = project.connect_ports(
        address(PortOwner::Composition(nested_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;

    let rendered = frame(&project, 0)?;
    let merge_group = find_group(&rendered.items, merge_id).context("Merge group must render")?;
    assert_eq!(merge_group.kind, FrameGroupKind::Merge);
    assert_eq!(merge_group.items.len(), 1);
    let connected_group = find_group(&merge_group.items, connection_id)
        .context("connected image wrapper must render")?;
    assert_eq!(connected_group.kind, FrameGroupKind::ConnectedImage);
    let nested_group =
        find_group(&connected_group.items, nested_id).context("nested Composition must render")?;
    assert_eq!(nested_group.kind, FrameGroupKind::Composition);
    assert_eq!((nested_group.width, nested_group.height), (800, 450));
    assert_eq!(nested_group.background_color, nested_background);
    assert!(nested_group.items.is_empty());
    Ok(())
}

#[test]
fn merge_skips_a_disabled_first_input_and_normalizes_the_first_produced_wire_at_runtime()
-> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();

    let inactive_clip = Clip::new("disabled first", 0.0, 2.0);
    let inactive_clip_id = inactive_clip.id;
    project.add_clip(inactive_clip);
    project.attach_clip_to_track(track_id, inactive_clip_id)?;
    project
        .get_clip_mut(inactive_clip_id)
        .context("inactive Clip must exist")?
        .blend_mode = BlendMode::Multiply;
    let inactive_node_id = add_node(
        &mut project,
        NodeContainer::Clip(inactive_clip_id),
        solid_node("inactive source"),
    )?;
    project
        .get_node_mut(inactive_node_id)
        .context("inactive Node must exist")?
        .enabled = false;
    project
        .set_output_node(
            NodeContainer::Clip(inactive_clip_id),
            Some(inactive_node_id),
        )
        .map_err(|error| anyhow!(error))?;

    let active_clip = Clip::new("active second", 0.0, 2.0);
    let active_clip_id = active_clip.id;
    project.add_clip(active_clip);
    project.attach_clip_to_track(track_id, active_clip_id)?;
    project
        .get_clip_mut(active_clip_id)
        .context("active Clip must exist")?
        .blend_mode = BlendMode::Overlay;
    let active_node_id = add_node(
        &mut project,
        NodeContainer::Clip(active_clip_id),
        solid_node("active source"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(active_clip_id), Some(active_node_id))
        .map_err(|error| anyhow!(error))?;

    let merge_id = structural_merge_id(&project, NodeContainer::Track(track_id))?;
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let inactive_connection_id = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Clip(inactive_clip_id) && connection.to == target
        })
        .context("inactive Clip structural edge must exist")?
        .id;
    let active_connection_id = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Clip(active_clip_id) && connection.to == target
        })
        .context("active Clip structural edge must exist")?
        .id;
    project.set_connection_blend_mode(inactive_connection_id, BlendMode::LinearDodge)?;
    project.set_connection_blend_mode(active_connection_id, BlendMode::Screen)?;

    let project_before_render = project.clone();
    let serialized_connections_before = serde_json::to_value(&project.connections)?;

    let rendered = frame(&project, 0)?;
    let merge = find_group(&rendered.items, merge_id).context("Merge group must render")?;
    assert_eq!(merge.items.len(), 1);
    let FrameItem::Group(active_wrapper) = &merge.items[0] else {
        bail!("a produced Merge input must be wrapped as a connected image");
    };
    assert_eq!(active_wrapper.source_id, active_connection_id);
    assert_eq!(active_wrapper.blend_mode, BlendMode::Normal);
    assert!(find_group(&active_wrapper.items, active_node_id).is_some());
    assert_eq!(object_source_ids(&merge.items), vec![active_node_id]);
    assert_eq!(project, project_before_render);
    assert_eq!(
        serde_json::to_value(&project.connections)?,
        serialized_connections_before,
        "base-layer normalization is runtime-only and must not rewrite wire blend metadata",
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == inactive_connection_id)
            .context("inactive connection must exist")?
            .blend_mode,
        BlendMode::LinearDodge,
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == active_connection_id)
            .context("active connection must exist")?
            .blend_mode,
        BlendMode::Screen,
    );

    // At ten seconds both Clips are inactive, so Merge and the root
    // Composition materialize as an empty background-only frame.
    assert!(frame(&project, 300)?.items.is_empty());
    Ok(())
}
