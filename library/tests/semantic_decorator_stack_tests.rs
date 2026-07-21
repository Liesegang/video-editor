use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Context, Result};
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, NodeContainer, NodeGraphBundle,
    PortAddress, PortOwner, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::{Clip, Composition, Node, NodeContent, Project};
use library::plugin::{PluginManager, SHAPE_TRANSFORM_COMPONENT_ID, property_port_key};
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
    transform_id: Uuid,
    style_ids: Vec<Uuid>,
}

fn fixture() -> Result<Fixture> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let graph = factory.create_shape_graph("M 0 0 H 40 V 20 Z", 320, 180, 40, 20)?;
    let mut project = Project::new("decorator stack");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("shape", 0.0, 2.0);
    let clip_id = clip.id;
    let owner = NodeContainer::Clip(clip_id);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(owner, graph)?;
    let transform_id = operation_nodes(&project, owner, |operation| {
        operation.component_id == SHAPE_TRANSFORM_COMPONENT_ID
    })
    .into_iter()
    .next()
    .context("Shape Transform exists")?;
    let mut style_ids = operation_nodes(&project, owner, |operation| operation.category == "style");
    style_ids.sort_unstable();
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    Ok(Fixture {
        shared,
        manager,
        owner,
        transform_id,
        style_ids,
    })
}

fn operation_nodes(
    project: &Project,
    owner: NodeContainer,
    predicate: impl Fn(&library::model::PluginOperationContent) -> bool,
) -> Vec<Uuid> {
    let ids = match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.node_ids.as_slice()),
        NodeContainer::Track(id) => project.get_track(id).map(|track| track.node_ids.as_slice()),
        NodeContainer::Clip(id) => project.get_clip(id).map(|clip| clip.node_ids.as_slice()),
    };
    ids.unwrap_or_default()
        .iter()
        .filter_map(|node_id| {
            let NodeContent::PluginOperation(operation) = project.get_node(*node_id)?.content()
            else {
                return None;
            };
            predicate(operation).then_some(*node_id)
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
fn append_reorder_remove_rewire_only_primary_shape_flow() -> Result<()> {
    let fixture = fixture()?;
    let style_input_ids = {
        let project = read(&fixture.shared)?;
        fixture
            .style_ids
            .iter()
            .map(|style_id| {
                project
                    .connections
                    .iter()
                    .find(|connection| {
                        connection.to
                            == PortAddress::new(PortOwner::Node(*style_id), SHAPE_INPUT_PORT)
                    })
                    .map(|connection| connection.id)
                    .with_context(|| format!("Style {style_id} Shape input"))
            })
            .collect::<Result<Vec<_>>>()?
    };
    let first = fixture
        .manager
        .append_semantic_container_decorator(fixture.owner, "backplate")?;
    let second = fixture
        .manager
        .append_semantic_container_decorator(fixture.owner, "backplate")?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_decorator_stack(fixture.owner)?
            .node_ids(),
        &[first, second]
    );

    let background_first = fixture.manager.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 5 V 5 Z".to_string(),
        },
        320,
        180,
        5,
        5,
    )?;
    let background_second = fixture.manager.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 8 V 8 Z".to_string(),
        },
        320,
        180,
        8,
        8,
    )?;
    let (first_property, second_property, first_background, second_background) = {
        let mut project = write(&fixture.shared)?;
        let first_driver = Node::new_add("first padding");
        let first_driver_id = first_driver.id;
        let second_driver = Node::new_add("second padding");
        let second_driver_id = second_driver.id;
        let first_background_id = background_first.id;
        let second_background_id = background_second.id;
        for node in [
            first_driver,
            second_driver,
            background_first,
            background_second,
        ] {
            let node_id = node.id;
            project.add_node(node);
            project.attach_node_to_container(fixture.owner, node_id)?;
        }
        let first_property = project.connect_ports(
            PortAddress::new(PortOwner::Node(first_driver_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(first), property_port_key("padding")),
        )?;
        let second_property = project.connect_ports(
            PortAddress::new(PortOwner::Node(second_driver_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(second), property_port_key("padding")),
        )?;
        let first_background = project.connect_ports(
            PortAddress::new(PortOwner::Node(first_background_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(first), BACKGROUND_SHAPE_INPUT_PORT),
        )?;
        let second_background = project.connect_ports(
            PortAddress::new(PortOwner::Node(second_background_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(second), BACKGROUND_SHAPE_INPUT_PORT),
        )?;
        (
            first_property,
            second_property,
            first_background,
            second_background,
        )
    };

    let before = read(&fixture.shared)?.clone();
    let decorator_nodes = [first, second]
        .into_iter()
        .map(|node_id| {
            before
                .get_node(node_id)
                .cloned()
                .map(|node| (node_id, node))
                .with_context(|| format!("Decorator {node_id}"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let connections_before = connection_state(&before);
    let main_ids = before
        .connections
        .iter()
        .filter(|connection| {
            connection.from
                == PortAddress::new(PortOwner::Node(fixture.transform_id), SHAPE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(first), SHAPE_INPUT_PORT)
                || connection.from == PortAddress::new(PortOwner::Node(first), SHAPE_OUTPUT_PORT)
                    && connection.to == PortAddress::new(PortOwner::Node(second), SHAPE_INPUT_PORT)
                || style_input_ids.contains(&connection.id)
        })
        .map(|connection| connection.id)
        .collect::<BTreeSet<_>>();

    fixture
        .manager
        .reorder_semantic_container_decorators(fixture.owner, &[second, first])?;
    let reordered = read(&fixture.shared)?.clone();
    assert_eq!(
        fixture
            .manager
            .semantic_container_decorator_stack(fixture.owner)?
            .node_ids(),
        &[second, first]
    );
    assert_eq!(
        [first, second]
            .into_iter()
            .map(|node_id| {
                reordered
                    .get_node(node_id)
                    .cloned()
                    .map(|node| (node_id, node))
                    .with_context(|| format!("Decorator {node_id}"))
            })
            .collect::<Result<BTreeMap<_, _>>>()?,
        decorator_nodes
    );
    let connections_after = connection_state(&reordered);
    assert_eq!(
        connections_before.keys().collect::<Vec<_>>(),
        connections_after.keys().collect::<Vec<_>>()
    );
    for (connection_id, original) in &connections_before {
        let current = connections_after
            .get(connection_id)
            .context("connection survives reorder")?;
        if main_ids.contains(connection_id) {
            assert_eq!((current.2, current.3), (original.2, original.3));
        } else {
            assert_eq!(current, original, "non-main wire {connection_id} changed");
        }
    }
    for connection_id in [
        first_property,
        second_property,
        first_background,
        second_background,
    ] {
        assert_eq!(
            connections_after.get(&connection_id),
            connections_before.get(&connection_id)
        );
    }
    for connection_id in &style_input_ids {
        let connection = reordered
            .connections
            .iter()
            .find(|connection| connection.id == *connection_id)
            .context("Style input survives")?;
        assert_eq!(
            connection.from,
            PortAddress::new(PortOwner::Node(first), SHAPE_OUTPUT_PORT)
        );
    }

    fixture
        .manager
        .reorder_semantic_container_decorators(fixture.owner, &[second, first])?;
    assert_eq!(*read(&fixture.shared)?, reordered);

    fixture
        .manager
        .remove_semantic_container_decorator(fixture.owner, first)?;
    let removed = read(&fixture.shared)?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_decorator_stack(fixture.owner)?
            .node_ids(),
        &[second]
    );
    for connection_id in style_input_ids {
        let connection = removed
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .context("Style input survives delete")?;
        assert_eq!(
            connection.from,
            PortAddress::new(PortOwner::Node(second), SHAPE_OUTPUT_PORT)
        );
    }
    assert_eq!(removed.get_node(second), reordered.get_node(second));
    assert!(removed.connections.iter().any(|connection| {
        connection.id == second_property || connection.id == second_background
    }));
    Ok(())
}

#[test]
fn separated_output_reaching_decorators_fail_without_mutation() -> Result<()> {
    let fixture = fixture()?;
    let decorator = fixture
        .manager
        .append_semantic_container_decorator(fixture.owner, "backplate")?;
    {
        let mut project = write(&fixture.shared)?;
        let transform = fixture
            .manager
            .get_plugin_manager()
            .create_shape_transform_operation_node()?;
        let transform_id = transform.id;
        project.add_node(transform);
        project.attach_node_to_container(fixture.owner, transform_id)?;
        for style_id in &fixture.style_ids {
            let connection = project
                .connections
                .iter_mut()
                .find(|connection| {
                    connection.to == PortAddress::new(PortOwner::Node(*style_id), SHAPE_INPUT_PORT)
                })
                .context("Style input")?;
            connection.from = PortAddress::new(PortOwner::Node(transform_id), SHAPE_OUTPUT_PORT);
        }
        project.connect_ports(
            PortAddress::new(PortOwner::Node(decorator), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
        )?;
        assert!(project.validate_connections().is_empty());
    }
    let before = read(&fixture.shared)?.clone();
    assert!(
        fixture
            .manager
            .semantic_container_decorator_stack(fixture.owner)
            .is_err()
    );
    assert!(
        fixture
            .manager
            .append_semantic_container_decorator(fixture.owner, "backplate")
            .is_err()
    );
    assert_eq!(*read(&fixture.shared)?, before);
    Ok(())
}

#[test]
fn decorator_can_be_authored_before_the_first_style() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let shape = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 10 V 10 Z".to_string(),
        },
        320,
        180,
        10,
        10,
    )?;
    let shape_id = shape.id;
    let mut project = Project::new("pre-style decorator");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 1.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("shape", 0.0, 1.0);
    let clip_id = clip.id;
    let owner = NodeContainer::Clip(clip_id);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(owner, NodeGraphBundle::new(vec![shape], Vec::new(), None))?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(shared, plugins);
    let decorator = manager.append_semantic_container_decorator(owner, "backplate")?;
    assert_eq!(
        manager
            .semantic_container_decorator_stack(owner)?
            .node_ids(),
        &[decorator]
    );
    let project = manager.get_project();
    let project = read(&project)?;
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(decorator), SHAPE_INPUT_PORT)
    }));
    Ok(())
}

#[test]
fn ordinary_backplate_branch_does_not_make_clip_decorator_facade_unusable() -> Result<()> {
    let fixture = fixture()?;
    fixture
        .manager
        .add_decorator(fixture.transform_id, "backplate")?;

    // A Backplate intentionally creates a second Shape -> Style anchor which
    // joins the original image at Merge. The semantic facade must discover
    // anchored Decorator chains independently instead of requiring every
    // output-reaching Style to share one immediate Shape source.
    let stack = fixture
        .manager
        .semantic_container_decorator_stack(fixture.owner)?;
    assert_eq!(stack.node_ids().len(), 1, "Backplate remains discoverable");

    let appended = fixture
        .manager
        .append_semantic_container_decorator(fixture.owner, "backplate")?;
    assert!(
        fixture
            .manager
            .semantic_container_decorator_stack(fixture.owner)?
            .node_ids()
            .contains(&appended)
    );
    Ok(())
}
