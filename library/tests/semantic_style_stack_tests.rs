use std::collections::BTreeMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Context, Result};
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::model::frame::color::Color;
use library::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    NUMBER_RESULT_OUTPUT_PORT, NodeContainer, NodeGraphBundle, PortAddress, PortOwner,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::{BlendMode, Clip, Composition, Node, NodeContent, Project};
use library::plugin::{PluginManager, property_port_key};
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
    fill_id: Uuid,
    effect_id: Uuid,
}

fn fixture() -> Result<Fixture> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let graph = factory.create_text_graph("Title", "Arial", 320, 180)?;
    let mut project = Project::new("style stack");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("title", 0.0, 2.0);
    let clip_id = clip.id;
    let owner = NodeContainer::Clip(clip_id);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(owner, graph)?;
    let fill_id = operation_nodes(&project, owner, "style", "fill")
        .into_iter()
        .next()
        .context("Fill exists")?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    let effect_id = manager.append_semantic_container_effect(owner, "blur")?;
    Ok(Fixture {
        shared,
        manager,
        owner,
        fill_id,
        effect_id,
    })
}

fn operation_nodes(
    project: &Project,
    owner: NodeContainer,
    category: &str,
    component: &str,
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
            (operation.category == category && operation.component_id == component)
                .then_some(*node_id)
        })
        .collect()
}

fn connection_state(
    project: &Project,
) -> BTreeMap<Uuid, (PortAddress, PortAddress, i64, BlendMode)> {
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
fn second_style_synthesizes_merge_and_reorder_preserves_branch_identity() -> Result<()> {
    let fixture = fixture()?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_style_stack(fixture.owner)?
            .node_ids(),
        &[fixture.fill_id]
    );
    let (boundary_id, fill_before) = {
        let project = read(&fixture.shared)?;
        let boundary_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from
                    == PortAddress::new(PortOwner::Node(fixture.fill_id), IMAGE_OUTPUT_PORT)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(fixture.effect_id), IMAGE_INPUT_PORT)
            })
            .context("Fill -> Effect boundary")?
            .id;
        let fill = project
            .get_node(fixture.fill_id)
            .cloned()
            .context("Fill exists")?;
        (boundary_id, fill)
    };
    let stroke = fixture
        .manager
        .append_semantic_container_style(fixture.owner, "stroke")?;
    let stack = fixture
        .manager
        .semantic_container_style_stack(fixture.owner)?;
    assert_eq!(stack.node_ids(), &[fixture.fill_id, stroke]);
    let merge_id = stack.merge_node_id().context("Style Merge synthesized")?;
    {
        let project = read(&fixture.shared)?;
        assert_eq!(project.get_node(fixture.fill_id), Some(&fill_before));
        let boundary = project
            .connections
            .iter()
            .find(|connection| connection.id == boundary_id)
            .context("existing downstream boundary survives")?;
        assert_eq!(
            boundary.from,
            PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT)
        );
        assert_eq!(
            boundary.to,
            PortAddress::new(PortOwner::Node(fixture.effect_id), IMAGE_INPUT_PORT)
        );
    }
    let second_fill = fixture
        .manager
        .append_semantic_container_style(fixture.owner, "fill")?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_style_stack(fixture.owner)?
            .node_ids(),
        &[fixture.fill_id, stroke, second_fill]
    );
    assert_eq!(
        fixture
            .manager
            .semantic_container_style_stack(fixture.owner)?
            .merge_node_id(),
        Some(merge_id),
        "third Style must reuse the unique Merge"
    );

    let property_wires = {
        let mut project = write(&fixture.shared)?;
        let properties = [
            (fixture.fill_id, "offset"),
            (stroke, "width"),
            (second_fill, "offset"),
        ];
        let mut wire_ids = Vec::new();
        for (index, (style_id, property)) in properties.into_iter().enumerate() {
            let driver = Node::new_add(&format!("Style driver {index}"));
            let driver_id = driver.id;
            project.add_node(driver);
            project.attach_node_to_container(fixture.owner, driver_id)?;
            wire_ids.push(project.connect_ports(
                PortAddress::new(PortOwner::Node(driver_id), NUMBER_RESULT_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(style_id), property_port_key(property)),
            )?);
        }
        let modes = [BlendMode::Multiply, BlendMode::Screen, BlendMode::Overlay];
        for (style_id, mode) in [fixture.fill_id, stroke, second_fill]
            .into_iter()
            .zip(modes)
        {
            let connection_id = project
                .connections
                .iter()
                .find(|connection| {
                    connection.from
                        == PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT)
                        && connection.to
                            == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
                })
                .context("Style Merge input")?
                .id;
            project.set_connection_blend_mode(connection_id, mode)?;
        }
        wire_ids
    };
    let before = read(&fixture.shared)?.clone();
    let nodes_before = [fixture.fill_id, stroke, second_fill]
        .into_iter()
        .map(|node_id| {
            before
                .get_node(node_id)
                .cloned()
                .map(|node| (node_id, node))
                .with_context(|| format!("Style {node_id}"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let connections_before = connection_state(&before);
    let merge_wire_by_style = [fixture.fill_id, stroke, second_fill]
        .into_iter()
        .map(|style_id| {
            before
                .connections
                .iter()
                .find(|connection| {
                    connection.from
                        == PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT)
                        && connection.to
                            == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
                })
                .map(|connection| (style_id, connection.id, connection.blend_mode))
                .with_context(|| format!("Style {style_id} Merge wire"))
        })
        .collect::<Result<Vec<_>>>()?;

    fixture.manager.reorder_semantic_container_styles(
        fixture.owner,
        &[second_fill, fixture.fill_id, stroke],
    )?;
    let reordered = read(&fixture.shared)?.clone();
    assert_eq!(
        fixture
            .manager
            .semantic_container_style_stack(fixture.owner)?
            .node_ids(),
        &[second_fill, fixture.fill_id, stroke]
    );
    assert_eq!(
        [fixture.fill_id, stroke, second_fill]
            .into_iter()
            .map(|node_id| {
                reordered
                    .get_node(node_id)
                    .cloned()
                    .map(|node| (node_id, node))
                    .with_context(|| format!("Style {node_id}"))
            })
            .collect::<Result<BTreeMap<_, _>>>()?,
        nodes_before
    );
    for (style_id, connection_id, blend_mode) in merge_wire_by_style {
        let connection = reordered
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .context("Merge wire survives reorder")?;
        assert_eq!(
            connection.from,
            PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT)
        );
        assert_eq!(connection.blend_mode, blend_mode);
    }
    let connections_after = connection_state(&reordered);
    for connection_id in property_wires {
        assert_eq!(
            connections_after.get(&connection_id),
            connections_before.get(&connection_id),
            "property wire changed during Style reorder"
        );
    }
    fixture.manager.reorder_semantic_container_styles(
        fixture.owner,
        &[second_fill, fixture.fill_id, stroke],
    )?;
    assert_eq!(*read(&fixture.shared)?, reordered);

    fixture
        .manager
        .remove_semantic_container_style(fixture.owner, stroke)?;
    let removed = read(&fixture.shared)?;
    assert_eq!(
        fixture
            .manager
            .semantic_container_style_stack(fixture.owner)?
            .node_ids(),
        &[second_fill, fixture.fill_id]
    );
    assert_eq!(
        removed.get_node(fixture.fill_id),
        reordered.get_node(fixture.fill_id)
    );
    assert_eq!(
        removed.get_node(second_fill),
        reordered.get_node(second_fill)
    );
    Ok(())
}

#[test]
fn zero_to_one_style_uses_terminal_decorator_and_last_direct_style_can_be_removed() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let shape = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 20 V 20 Z".to_string(),
        },
        320,
        180,
        20,
        20,
    )?;
    let background = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 8 V 8 Z".to_string(),
        },
        320,
        180,
        8,
        8,
    )?;
    let shape_id = shape.id;
    let background_id = background.id;
    let mut project = Project::new("zero style");
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
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    // Add the Decorator while the main Shape is still unambiguous, then mark
    // the other Shape as its explicit auxiliary geometry.
    let decorator = manager.append_semantic_container_decorator(owner, "backplate")?;
    {
        let mut project = write(&shared)?;
        project.add_node(background);
        project.attach_node_to_container(owner, background_id)?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(background_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(decorator), BACKGROUND_SHAPE_INPUT_PORT),
        )?;
    }
    let fill = manager.append_semantic_container_style(owner, "fill")?;
    assert_eq!(
        manager.semantic_container_style_stack(owner)?.node_ids(),
        &[fill]
    );
    {
        let project = read(&shared)?;
        assert!(project.connections.iter().any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(decorator), SHAPE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(fill), SHAPE_INPUT_PORT)
        }));
        assert!(project.connections.iter().any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(decorator), SHAPE_INPUT_PORT)
        }));
    }
    manager.remove_semantic_container_style(owner, fill)?;
    assert!(
        manager
            .semantic_container_style_stack(owner)?
            .node_ids()
            .is_empty()
    );
    assert_eq!(
        read(&shared)?
            .get_clip(clip_id)
            .context("Clip exists")?
            .output_node_id,
        None
    );
    let stroke = manager.append_semantic_container_style(owner, "stroke")?;
    assert_eq!(
        manager.semantic_container_style_stack(owner)?.node_ids(),
        &[stroke]
    );
    Ok(())
}

#[test]
fn mixed_merge_inputs_survive_style_reorder_and_last_style_removal() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let graph = factory.create_shape_graph("M 0 0 H 20 V 20 Z", 320, 180, 20, 20)?;
    let mut project = Project::new("mixed style merge");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 1.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("shape", 0.0, 1.0);
    let clip_id = clip.id;
    let owner = NodeContainer::Clip(clip_id);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(owner, graph)?;
    let merge_id = project
        .get_clip(clip_id)
        .and_then(|clip| clip.output_node_id)
        .context("Merge output")?;
    let image = factory.create_generator_node(
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
        320,
        180,
        320,
        180,
    )?;
    let image_id = image.id;
    project.add_node(image);
    project.attach_node_to_container(owner, image_id)?;
    let non_style_wire = project.connect_ports(
        PortAddress::new(PortOwner::Node(image_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    project.set_connection_blend_mode(non_style_wire, BlendMode::Difference)?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    let stack = manager.semantic_container_style_stack(owner)?;
    assert_eq!(stack.node_ids().len(), 2);
    let first = stack.node_ids()[0];
    let second = stack.node_ids()[1];
    let non_style_before = read(&shared)?
        .connections
        .iter()
        .find(|connection| connection.id == non_style_wire)
        .cloned()
        .context("non-Style Merge wire")?;
    manager.reorder_semantic_container_styles(owner, &[second, first])?;
    let non_style_after = read(&shared)?
        .connections
        .iter()
        .find(|connection| connection.id == non_style_wire)
        .cloned()
        .context("non-Style Merge wire survives reorder")?;
    assert_eq!(non_style_after, non_style_before);

    let inserted = manager.append_semantic_container_style_after(owner, "fill", Some(first))?;
    let inserted_branch = manager
        .semantic_container_style_stack(owner)?
        .branches()
        .iter()
        .find(|branch| branch.node_id() == inserted)
        .cloned()
        .context("inserted Style branch")?;
    let first_branch = manager
        .semantic_container_style_stack(owner)?
        .branches()
        .iter()
        .find(|branch| branch.node_id() == first)
        .cloned()
        .context("anchor Style branch")?;
    assert_eq!(inserted_branch.shape_source(), first_branch.shape_source());

    let before_invalid_order = read(&shared)?.clone();
    assert!(
        manager
            .reorder_semantic_container_styles(owner, &[first, first])
            .is_err()
    );
    assert_eq!(*read(&shared)?, before_invalid_order);

    for style_id in [second, inserted, first] {
        manager.remove_semantic_container_style(owner, style_id)?;
    }
    let project = read(&shared)?;
    assert!(
        manager
            .semantic_container_style_stack(owner)?
            .node_ids()
            .is_empty()
    );
    assert!(project.get_node(merge_id).is_some());
    assert!(project.get_node(image_id).is_some());
    let non_style_final = project
        .connections
        .iter()
        .find(|connection| connection.id == non_style_wire)
        .context("non-Style wire survives Style removal")?;
    assert_eq!(non_style_final.id, non_style_before.id);
    assert_eq!(non_style_final.from, non_style_before.from);
    assert_eq!(non_style_final.to, non_style_before.to);
    assert_eq!(non_style_final.blend_mode, non_style_before.blend_mode);
    assert_eq!(
        project
            .get_clip(clip_id)
            .context("Clip exists")?
            .output_node_id,
        Some(merge_id)
    );
    Ok(())
}

#[test]
fn distinct_text_and_backplate_shape_branches_require_an_anchored_add() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let text = factory.create_text_node("Title", "Arial", 320, 180)?;
    let background = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 40 V 20 Z".to_string(),
        },
        320,
        180,
        40,
        20,
    )?;
    let backplate = plugins.create_decorator_operation_node("backplate")?;
    let main_fill = plugins.create_style_operation_node("fill")?;
    let backplate_fill = plugins.create_style_operation_node("fill")?;
    let merge = Node::new_merge("Text + Backplate");
    let text_id = text.id;
    let background_id = background.id;
    let backplate_id = backplate.id;
    let main_fill_id = main_fill.id;
    let backplate_fill_id = backplate_fill.id;
    let merge_id = merge.id;

    let mut project = Project::new("distinct style branches");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 1.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("title", 0.0, 1.0);
    let clip_id = clip.id;
    let owner = NodeContainer::Clip(clip_id);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(
        owner,
        NodeGraphBundle::new(
            vec![
                text,
                background,
                backplate,
                main_fill,
                backplate_fill,
                merge,
            ],
            vec![
                library::model::project::ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(main_fill_id), SHAPE_INPUT_PORT),
                    0,
                ),
                library::model::project::ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(backplate_id), SHAPE_INPUT_PORT),
                    0,
                ),
                library::model::project::ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(background_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(backplate_id), BACKGROUND_SHAPE_INPUT_PORT),
                    0,
                ),
                library::model::project::ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(backplate_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(backplate_fill_id), SHAPE_INPUT_PORT),
                    0,
                ),
                library::model::project::ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(main_fill_id), IMAGE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                    0,
                ),
                library::model::project::ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(backplate_fill_id), IMAGE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                    1,
                ),
            ],
            Some(merge_id),
        ),
    )?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);

    let stack = manager.semantic_container_style_stack(owner)?;
    assert_eq!(stack.node_ids(), &[main_fill_id, backplate_fill_id]);
    assert_ne!(
        stack.branches()[0].shape_source(),
        stack.branches()[1].shape_source()
    );
    let before_unanchored = read(&shared)?.clone();
    let error = manager
        .append_semantic_container_style(owner, "stroke")
        .expect_err("ambiguous source must fail closed");
    assert!(error.to_string().contains("pass after_style_id"));
    assert_eq!(*read(&shared)?, before_unanchored);

    let stroke =
        manager.append_semantic_container_style_after(owner, "stroke", Some(backplate_fill_id))?;
    let stack = manager.semantic_container_style_stack(owner)?;
    assert_eq!(stack.node_ids(), &[main_fill_id, backplate_fill_id, stroke]);
    let stroke_branch = stack
        .branches()
        .iter()
        .find(|branch| branch.node_id() == stroke)
        .context("Stroke branch")?;
    assert_eq!(
        stroke_branch.shape_source(),
        &PortAddress::new(PortOwner::Node(backplate_id), SHAPE_OUTPUT_PORT)
    );
    Ok(())
}
