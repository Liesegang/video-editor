use super::*;

#[test]
fn container_ports_separate_authored_inputs_from_read_only_runtime_outputs() -> Result<()> {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;

    for owner in [
        PortOwner::Composition(composition_id),
        PortOwner::Track(track_id),
        PortOwner::Clip(clip_id),
    ] {
        let ports = project.port_definitions(owner);
        assert_eq!(ports.len(), 10);
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (DURATION_PORT, PortDataType::Number),
            (RESOLUTION_PORT, PortDataType::Vec2),
        ] {
            let input = ports
                .iter()
                .find(|port| port.key == key && port.direction == PortDirection::Input)
                .with_context(|| format!("{key} input port must exist"))?;
            assert_eq!(input.side, PortSide::Left);
            assert_eq!(input.exposure, PortExposure::External);
            assert_eq!(input.data_type, data_type);
        }
        assert!(!ports.iter().any(|port| {
            port.direction == PortDirection::Input
                && matches!(port.key.as_str(), FRAME_PORT | FPS_PORT)
        }));
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (FRAME_PORT, PortDataType::Integer),
            (FPS_PORT, PortDataType::Number),
            (DURATION_PORT, PortDataType::Number),
            (RESOLUTION_PORT, PortDataType::Vec2),
        ] {
            let output = ports
                .iter()
                .find(|port| port.key == key && port.direction == PortDirection::Output)
                .with_context(|| format!("{key} output port must exist"))?;
            assert_eq!(output.side, PortSide::Left);
            assert_eq!(output.exposure, PortExposure::Internal);
            assert_eq!(output.data_type, data_type);
        }
        assert_external_container_output(&ports, IMAGE_OUTPUT_PORT, PortDataType::Image)?;
        assert_external_container_output(&ports, AUDIO_OUTPUT_PORT, PortDataType::Audio)?;
    }
    Ok(())
}

#[test]
fn cross_track_image_connection_preserves_containment_and_internal_metadata_cannot_escape()
-> Result<()> {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, second_track_id)?;
    let first_clip = add_clip(&mut project, first_track_id, "source clip")?;
    let second_clip = add_clip(&mut project, second_track_id, "target clip")?;
    let source_id = add_node(
        &mut project,
        NodeContainer::Clip(first_clip),
        solid_node("source"),
    )?;
    let transform_id = add_node(
        &mut project,
        NodeContainer::Clip(second_clip),
        PluginManager::default().create_image_transform_operation_node()?,
    )?;

    project.connect_ports(
        address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
    )?;
    assert_eq!(
        project.find_node_container(source_id),
        Some(NodeContainer::Clip(first_clip))
    );
    assert_eq!(
        project.find_node_container(transform_id),
        Some(NodeContainer::Clip(second_clip))
    );

    let escaped = project.connect_ports(
        address(PortOwner::Composition(composition_id), TIME_PORT),
        address(PortOwner::Node(transform_id), TIME_PORT),
    );
    assert_eq!(
        escaped,
        Err(ProjectGraphError::InternalPortEscapesContainer {
            source_owner: PortOwner::Composition(composition_id),
            target_owner: PortOwner::Node(transform_id),
        })
    );
    Ok(())
}
