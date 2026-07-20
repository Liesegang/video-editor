use super::*;

pub(super) const E2E_AMBIGUOUS_CLIP_ID: Uuid = Uuid::from_u128(0x304);
pub(super) const E2E_AMBIGUOUS_SHAPE_A_ID: Uuid = Uuid::from_u128(0x410);
pub(super) const E2E_AMBIGUOUS_SHAPE_B_ID: Uuid = Uuid::from_u128(0x411);
pub(super) const E2E_AMBIGUOUS_TRANSFORM_A_ID: Uuid = Uuid::from_u128(0x510);
pub(super) const E2E_AMBIGUOUS_TRANSFORM_B_ID: Uuid = Uuid::from_u128(0x511);
pub(super) const E2E_AMBIGUOUS_FILL_A_ID: Uuid = Uuid::from_u128(0x610);
pub(super) const E2E_AMBIGUOUS_FILL_B_ID: Uuid = Uuid::from_u128(0x611);
pub(super) const E2E_AMBIGUOUS_MERGE_ID: Uuid = Uuid::from_u128(0x612);

pub(super) fn install(
    project: &mut Project,
    factory: &ProjectService,
    plugin_manager: &PluginManager,
) -> Result<(), String> {
    let mut clip = Clip::new("QA Ambiguous Transform Clip", 1.0, 8.0);
    clip.id = E2E_AMBIGUOUS_CLIP_ID;
    clip.node_ids = vec![
        E2E_AMBIGUOUS_SHAPE_A_ID,
        E2E_AMBIGUOUS_TRANSFORM_A_ID,
        E2E_AMBIGUOUS_FILL_A_ID,
        E2E_AMBIGUOUS_SHAPE_B_ID,
        E2E_AMBIGUOUS_TRANSFORM_B_ID,
        E2E_AMBIGUOUS_FILL_B_ID,
        E2E_AMBIGUOUS_MERGE_ID,
    ];
    clip.output_node_id = Some(E2E_AMBIGUOUS_MERGE_ID);
    clip.ui_position = [1900.0, 860.0];
    clip.ui_size = [1100.0, 380.0];

    let mut shape_a = shape_node(factory, E2E_AMBIGUOUS_SHAPE_A_ID, [1980.0, 980.0])?;
    shape_a.name = "QA Ambiguous Shape A".to_string();
    let mut shape_b = shape_node(factory, E2E_AMBIGUOUS_SHAPE_B_ID, [1980.0, 1120.0])?;
    shape_b.name = "QA Ambiguous Shape B".to_string();
    let transform_a = root_transform_node(
        plugin_manager,
        E2E_AMBIGUOUS_TRANSFORM_A_ID,
        "QA Ambiguous Transform A",
        [-140.0, -70.0],
        [80.0, 45.0],
        [2220.0, 940.0],
    )?;
    let transform_b = root_transform_node(
        plugin_manager,
        E2E_AMBIGUOUS_TRANSFORM_B_ID,
        "QA Ambiguous Transform B",
        [150.0, 80.0],
        [80.0, 45.0],
        [2220.0, 1100.0],
    )?;
    let mut fill_a = operation_node(
        plugin_manager.create_style_operation_node("fill"),
        E2E_AMBIGUOUS_FILL_A_ID,
        "QA Ambiguous Fill A",
        [2480.0, 940.0],
    )?;
    fill_a.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 255,
            g: 80,
            b: 170,
            a: 255,
        })),
    )?;
    fill_a.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    )?;
    let mut fill_b = operation_node(
        plugin_manager.create_style_operation_node("fill"),
        E2E_AMBIGUOUS_FILL_B_ID,
        "QA Ambiguous Fill B",
        [2480.0, 1100.0],
    )?;
    fill_b.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 70,
            g: 150,
            b: 255,
            a: 255,
        })),
    )?;
    fill_b.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    )?;
    let mut merge = Node::new_merge("QA Ambiguous Merge");
    merge.id = E2E_AMBIGUOUS_MERGE_ID;
    merge.ui_position = [2740.0, 1020.0];

    project.add_clip(clip);
    project
        .attach_clip_to_track(E2E_TRACK_B_ID, E2E_AMBIGUOUS_CLIP_ID)
        .map_err(|error| format!("cannot attach ambiguous QA Clip: {error}"))?;
    for node in [
        shape_a,
        transform_a,
        fill_a,
        shape_b,
        transform_b,
        fill_b,
        merge,
    ] {
        project.add_node(node);
    }

    for (source, target) in [
        (E2E_AMBIGUOUS_SHAPE_A_ID, E2E_AMBIGUOUS_TRANSFORM_A_ID),
        (E2E_AMBIGUOUS_TRANSFORM_A_ID, E2E_AMBIGUOUS_FILL_A_ID),
        (E2E_AMBIGUOUS_SHAPE_B_ID, E2E_AMBIGUOUS_TRANSFORM_B_ID),
        (E2E_AMBIGUOUS_TRANSFORM_B_ID, E2E_AMBIGUOUS_FILL_B_ID),
    ] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(target), SHAPE_INPUT_PORT),
            )
            .map_err(|error| format!("cannot connect ambiguous QA Shape graph: {error}"))?;
    }
    for source in [E2E_AMBIGUOUS_FILL_A_ID, E2E_AMBIGUOUS_FILL_B_ID] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(E2E_AMBIGUOUS_MERGE_ID), MERGE_IMAGES_PORT),
            )
            .map_err(|error| format!("cannot connect ambiguous QA Merge: {error}"))?;
    }
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(E2E_AMBIGUOUS_CLIP_ID), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(E2E_MERGE_ID), MERGE_IMAGES_PORT),
        )
        .map_err(|error| format!("cannot connect ambiguous QA Clip output: {error}"))?;
    for node in [
        E2E_AMBIGUOUS_SHAPE_A_ID,
        E2E_AMBIGUOUS_TRANSFORM_A_ID,
        E2E_AMBIGUOUS_FILL_A_ID,
        E2E_AMBIGUOUS_SHAPE_B_ID,
        E2E_AMBIGUOUS_TRANSFORM_B_ID,
        E2E_AMBIGUOUS_FILL_B_ID,
        E2E_AMBIGUOUS_MERGE_ID,
    ] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(E2E_AMBIGUOUS_CLIP_ID), TIME_PORT),
                PortAddress::new(PortOwner::Node(node), TIME_PORT),
            )
            .map_err(|error| format!("cannot connect ambiguous QA time metadata: {error}"))?;
    }
    Ok(())
}
