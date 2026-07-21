use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Context, Result};
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::model::frame::color::Color;
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NodeContainer, NodeGraphBundle, PortAddress, PortOwner,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, Composition, Node, NodeContent, Project};
use library::plugin::{
    EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID, PluginManager,
    property_port_key,
};
use uuid::Uuid;

fn read(project: &RwLock<Project>) -> Result<RwLockReadGuard<'_, Project>> {
    project
        .read()
        .map_err(|error| anyhow::anyhow!("Project read lock poisoned: {error}"))
}

fn write(project: &RwLock<Project>) -> Result<RwLockWriteGuard<'_, Project>> {
    project
        .write()
        .map_err(|error| anyhow::anyhow!("Project write lock poisoned: {error}"))
}

struct Fixture {
    shared: Arc<RwLock<Project>>,
    manager: ProjectManager,
    owner: NodeContainer,
    source_id: Uuid,
    transform_id: Uuid,
}

fn fixture() -> Result<Fixture> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let source = factory.create_generator_node(
        GeneratorNodeRequest::Solid {
            color: Color {
                r: 20,
                g: 40,
                b: 80,
                a: 255,
            },
        },
        320,
        180,
        320,
        180,
    )?;
    let source_id = source.id;
    let mut project = Project::new("semantic effects");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("solid", 0.0, 2.0);
    let clip_id = clip.id;
    let owner = NodeContainer::Clip(clip_id);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(
        owner,
        NodeGraphBundle::new(vec![source], Vec::new(), Some(source_id)),
    )?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    manager.update_semantic_container_property_or_keyframe(
        owner,
        "rotation",
        0.0,
        PropertyValue::from(0.0),
        None,
    )?;
    let transform_id = {
        let project = read(&shared)?;
        operation_id(&project, owner, IMAGE_TRANSFORM_COMPONENT_ID)
            .context("Image Transform was synthesized")?
    };
    Ok(Fixture {
        shared,
        manager,
        owner,
        source_id,
        transform_id,
    })
}

fn operation_id(project: &Project, owner: NodeContainer, component_id: &str) -> Option<Uuid> {
    let node_ids = match owner {
        NodeContainer::Composition(id) => project.get_composition(id)?.node_ids.as_slice(),
        NodeContainer::Track(id) => project.get_track(id)?.node_ids.as_slice(),
        NodeContainer::Clip(id) => project.get_clip(id)?.node_ids.as_slice(),
    };
    node_ids.iter().find_map(|node_id| {
        let NodeContent::PluginOperation(operation) = project.get_node(*node_id)?.content() else {
            return None;
        };
        (operation.component_id == component_id).then_some(*node_id)
    })
}

fn effect_nodes(project: &Project, ids: &[Uuid]) -> Result<BTreeMap<Uuid, Node>> {
    ids.iter()
        .map(|node_id| {
            project
                .get_node(*node_id)
                .cloned()
                .map(|node| (*node_id, node))
                .with_context(|| format!("Effect Node {node_id} exists"))
        })
        .collect()
}

fn connection_state(
    project: &Project,
) -> BTreeMap<Uuid, (PortAddress, PortAddress, i64, library::model::BlendMode)> {
    project
        .connections
        .iter()
        .map(|connection| {
            (
                connection.id,
                (
                    connection.from.clone(),
                    connection.to.clone(),
                    connection.order,
                    connection.blend_mode,
                ),
            )
        })
        .collect()
}

#[test]
fn append_reorder_delete_preserve_effect_identity_properties_and_non_main_wires() -> Result<()> {
    let fixture = fixture()?;
    let initial_boundary = {
        let project = read(&fixture.shared)?;
        project
            .connections
            .iter()
            .find(|connection| {
                connection.from
                    == PortAddress::new(PortOwner::Node(fixture.source_id), IMAGE_OUTPUT_PORT)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(fixture.transform_id), IMAGE_INPUT_PORT)
            })
            .context("source -> trailing Transform boundary")?
            .id
    };
    let blur = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "blur")?;
    let shadow = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "drop_shadow")?;
    let tile = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "tile")?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[blur, shadow, tile]
    );
    let (blur_property_wire, shadow_property_wire, blur_fanout_wire, shadow_fanout_wire) = {
        let mut project = write(&fixture.shared)?;
        project
            .get_node_mut(blur)
            .context("Blur exists")?
            .set_property(
                "sigma_x".to_string(),
                Property::expression("3.0 + time".to_string(), PropertyValue::from(3.0)),
            )
            .map_err(anyhow::Error::msg)?;
        let blur_driver = Node::new_add("blur sigma driver");
        let blur_driver_id = blur_driver.id;
        project.add_node(blur_driver);
        project.attach_node_to_container(fixture.owner, blur_driver_id)?;
        let blur_property_wire = project.connect_ports(
            PortAddress::new(PortOwner::Node(blur_driver_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(blur), property_port_key("sigma_y")),
        )?;
        let shadow_driver = Node::new_add("shadow dx driver");
        let shadow_driver_id = shadow_driver.id;
        project.add_node(shadow_driver);
        project.attach_node_to_container(fixture.owner, shadow_driver_id)?;
        let shadow_property_wire = project.connect_ports(
            PortAddress::new(PortOwner::Node(shadow_driver_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(shadow), property_port_key("dx")),
        )?;
        let blur_fanout = fixture
            .manager
            .get_plugin_manager()
            .create_image_transform_operation_node()?;
        let blur_fanout_id = blur_fanout.id;
        project.add_node(blur_fanout);
        project.attach_node_to_container(fixture.owner, blur_fanout_id)?;
        let blur_fanout_wire = project.connect_ports(
            PortAddress::new(PortOwner::Node(blur), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(blur_fanout_id), IMAGE_INPUT_PORT),
        )?;
        let shadow_fanout = fixture
            .manager
            .get_plugin_manager()
            .create_image_transform_operation_node()?;
        let shadow_fanout_id = shadow_fanout.id;
        project.add_node(shadow_fanout);
        project.attach_node_to_container(fixture.owner, shadow_fanout_id)?;
        let shadow_fanout_wire = project.connect_ports(
            PortAddress::new(PortOwner::Node(shadow), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(shadow_fanout_id), IMAGE_INPUT_PORT),
        )?;
        (
            blur_property_wire,
            shadow_property_wire,
            blur_fanout_wire,
            shadow_fanout_wire,
        )
    };
    let before_reorder = read(&fixture.shared)?.clone();
    let original_nodes = effect_nodes(&before_reorder, &[blur, shadow, tile])?;
    let original_connections = connection_state(&before_reorder);
    let main_connection_ids = [
        (fixture.source_id, blur),
        (blur, shadow),
        (shadow, tile),
        (tile, fixture.transform_id),
    ]
    .into_iter()
    .map(|(from, to)| {
        before_reorder
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Node(from), IMAGE_OUTPUT_PORT)
                    && connection.to == PortAddress::new(PortOwner::Node(to), IMAGE_INPUT_PORT)
            })
            .map(|connection| connection.id)
            .with_context(|| format!("main-flow connection {from} -> {to}"))
    })
    .collect::<Result<BTreeSet<_>>>()?;

    fixture
        .manager
        .reorder_semantic_container_effects(fixture.owner, &[tile, blur, shadow])?;
    let reordered = read(&fixture.shared)?.clone();
    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[tile, blur, shadow]
    );
    assert_eq!(
        effect_nodes(&reordered, &[blur, shadow, tile])?,
        original_nodes,
        "reorder must not replace or edit Effect Nodes"
    );
    let reordered_connections = connection_state(&reordered);
    assert_eq!(
        original_connections.keys().collect::<Vec<_>>(),
        reordered_connections.keys().collect::<Vec<_>>(),
        "reorder must not allocate or delete connections"
    );
    for (connection_id, original) in &original_connections {
        if !main_connection_ids.contains(connection_id) {
            assert_eq!(
                reordered_connections.get(connection_id),
                Some(original),
                "non-main wire {connection_id} changed"
            );
        }
    }
    for connection_id in &main_connection_ids {
        let original = original_connections
            .get(connection_id)
            .context("original main connection")?;
        let current = reordered_connections
            .get(connection_id)
            .context("reordered main connection")?;
        assert_eq!(
            (current.2, current.3),
            (original.2, original.3),
            "main-flow connection {connection_id} lost order/blend metadata"
        );
    }
    for connection_id in [
        blur_property_wire,
        shadow_property_wire,
        blur_fanout_wire,
        shadow_fanout_wire,
    ] {
        assert_eq!(
            reordered_connections.get(&connection_id),
            original_connections.get(&connection_id),
            "Effect-owned external wire {connection_id} changed endpoint"
        );
    }
    assert_eq!(
        reordered
            .connections
            .iter()
            .find(|connection| connection.id == initial_boundary)
            .context("original downstream boundary survives")?
            .from,
        PortAddress::new(PortOwner::Node(shadow), IMAGE_OUTPUT_PORT)
    );

    fixture
        .manager
        .reorder_semantic_container_effects(fixture.owner, &[tile, blur, shadow])?;
    assert_eq!(
        *read(&fixture.shared)?,
        reordered,
        "same-order reorder must be idempotent"
    );

    let before_delete = reordered;
    let outgoing_id = before_delete
        .connections
        .iter()
        .find(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(blur), IMAGE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(shadow), IMAGE_INPUT_PORT)
        })
        .context("Blur downstream main wire")?
        .id;
    let outgoing_metadata = before_delete
        .connections
        .iter()
        .find(|connection| connection.id == outgoing_id)
        .map(|connection| (connection.order, connection.blend_mode))
        .context("Blur downstream metadata")?;
    fixture
        .manager
        .remove_semantic_container_effect(fixture.owner, blur)?;
    let after_delete = read(&fixture.shared)?.clone();
    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[tile, shadow]
    );
    let bypass = after_delete
        .connections
        .iter()
        .find(|connection| connection.id == outgoing_id)
        .context("downstream connection UUID survives delete")?;
    assert_eq!(
        bypass.from,
        PortAddress::new(PortOwner::Node(tile), IMAGE_OUTPUT_PORT)
    );
    assert_eq!(
        bypass.to,
        PortAddress::new(PortOwner::Node(shadow), IMAGE_INPUT_PORT)
    );
    assert_eq!((bypass.order, bypass.blend_mode), outgoing_metadata);
    assert_eq!(
        effect_nodes(&after_delete, &[tile, shadow])?,
        effect_nodes(&before_delete, &[tile, shadow])?
    );

    // Project snapshots are the history payload. Restoring either side of
    // the one-operation transaction reproduces the exact semantic order.
    *write(&fixture.shared)? = before_delete;
    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[tile, blur, shadow]
    );
    *write(&fixture.shared)? = after_delete.clone();
    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[tile, shadow]
    );
    Ok(())
}

#[test]
fn invalid_reorder_and_output_reaching_branch_fail_without_partial_mutation() -> Result<()> {
    let fixture = fixture()?;
    let blur = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "blur")?;
    let shadow = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "drop_shadow")?;
    let before_invalid_order = read(&fixture.shared)?.clone();
    assert!(
        fixture
            .manager
            .reorder_semantic_container_effects(fixture.owner, &[blur, blur])
            .is_err()
    );
    assert_eq!(*read(&fixture.shared)?, before_invalid_order);

    {
        let mut project = write(&fixture.shared)?;
        let tile = fixture
            .manager
            .get_plugin_manager()
            .create_effect_operation_node("tile")?;
        let tile_id = tile.id;
        let merge = Node::new_merge("branched effects");
        let merge_id = merge.id;
        project.insert_node_graph(
            fixture.owner,
            NodeGraphBundle::new(vec![tile, merge], Vec::new(), None),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(blur), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(tile_id), IMAGE_INPUT_PORT),
        )?;
        let boundary = project
            .connections
            .iter_mut()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Node(shadow), IMAGE_OUTPUT_PORT)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(fixture.transform_id), IMAGE_INPUT_PORT)
            })
            .context("Effect tail boundary")?;
        boundary.from = PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT);
        project.connect_ports(
            PortAddress::new(PortOwner::Node(shadow), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(tile_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        assert!(project.validate_connections().is_empty());
    }
    let branched = read(&fixture.shared)?.clone();
    let error = fixture
        .manager
        .semantic_container_effect_stack(fixture.owner)
        .err()
        .context("branched output-reaching Effects must be ambiguous")?;
    assert!(error.to_string().contains("branch"));
    assert!(
        fixture
            .manager
            .append_semantic_container_effect(fixture.owner, "tile")
            .is_err()
    );
    assert_eq!(*read(&fixture.shared)?, branched);
    Ok(())
}

#[test]
fn added_effects_are_inserted_before_trailing_transform_and_opacity() -> Result<()> {
    let fixture = fixture()?;
    let opacity_id = {
        let project = read(&fixture.shared)?;
        operation_id(&project, fixture.owner, IMAGE_OPACITY_STYLE_COMPONENT_ID)
            .context("Image Opacity was synthesized")?
    };
    let blur = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "blur")?;
    let project = read(&fixture.shared)?;
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(fixture.source_id), IMAGE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(blur), IMAGE_INPUT_PORT)
    }));
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(blur), IMAGE_OUTPUT_PORT)
            && connection.to
                == PortAddress::new(PortOwner::Node(fixture.transform_id), IMAGE_INPUT_PORT)
    }));
    assert!(project.connections.iter().any(|connection| {
        connection.from
            == PortAddress::new(PortOwner::Node(fixture.transform_id), IMAGE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(opacity_id), IMAGE_INPUT_PORT)
    }));
    let effect = project.get_node(blur).context("Blur exists")?;
    assert!(matches!(
        effect.content(),
        NodeContent::PluginOperation(operation) if operation.category == EFFECT_CATEGORY
    ));
    Ok(())
}

#[test]
fn branch_local_effect_does_not_hide_the_post_merge_semantic_trunk() -> Result<()> {
    let fixture = fixture()?;
    let (branch_effect, trunk_effect) = {
        let mut project = write(&fixture.shared)?;
        let branch = fixture
            .manager
            .get_plugin_manager()
            .create_effect_operation_node("blur")?;
        let trunk = fixture
            .manager
            .get_plugin_manager()
            .create_effect_operation_node("tile")?;
        let merge = Node::new_merge("branch-local effect merge");
        let (branch_id, trunk_id, merge_id) = (branch.id, trunk.id, merge.id);
        project.insert_node_graph(
            fixture.owner,
            NodeGraphBundle::new(vec![branch, trunk, merge], Vec::new(), None),
        )?;
        let boundary = project
            .connections
            .iter_mut()
            .find(|connection| {
                connection.from
                    == PortAddress::new(PortOwner::Node(fixture.source_id), IMAGE_OUTPUT_PORT)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(fixture.transform_id), IMAGE_INPUT_PORT)
            })
            .context("source -> Transform boundary")?;
        boundary.from = PortAddress::new(PortOwner::Node(trunk_id), IMAGE_OUTPUT_PORT);
        project.connect_ports(
            PortAddress::new(PortOwner::Node(fixture.source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(branch_id), IMAGE_INPUT_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(branch_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(fixture.source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(trunk_id), IMAGE_INPUT_PORT),
        )?;
        assert!(project.validate_connections().is_empty());
        (branch_id, trunk_id)
    };
    let before = read(&fixture.shared)?.clone();

    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[trunk_effect],
        "the semantic stack is the contiguous post-Merge trunk"
    );
    let appended = fixture
        .manager
        .append_semantic_container_effect(fixture.owner, "drop_shadow")?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[trunk_effect, appended]
    );
    assert_eq!(
        read(&fixture.shared)?.get_node(branch_effect),
        before.get_node(branch_effect),
        "branch-local advanced editing is untouched"
    );
    Ok(())
}
