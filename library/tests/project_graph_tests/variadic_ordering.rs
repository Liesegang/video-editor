use anyhow::{Context, Result};
use library::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortOwner, Project,
    ProjectConnection, ProjectGraphError,
};
use library::model::{BlendMode, Node};
use uuid::Uuid;

use super::graph_support::{add_clip, add_node, address, project_with_composition, solid_node};

#[test]
fn malformed_serialized_variadic_orders_are_reported_without_repairing_the_model() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let source_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("source"),
    )?;
    let other_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("other"),
    )?;
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("merge"),
    )?;
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    project.connections = vec![
        ProjectConnection::new(
            address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            target.clone(),
            4,
        ),
        ProjectConnection::new(
            address(PortOwner::Node(other_id), IMAGE_OUTPUT_PORT),
            target.clone(),
            4,
        ),
    ];

    assert!(
        project
            .validate_connections()
            .contains(&ProjectGraphError::DuplicateConnectionOrder { target, order: 4 })
    );
    assert_eq!(project.connections[0].order, 4);
    assert_eq!(project.connections[1].order, 4);
    Ok(())
}

fn reverse_stored_duplicate_merge_project() -> Result<(Project, PortAddress, [Uuid; 3])> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let sources = ["low UUID", "middle UUID", "high UUID"]
        .into_iter()
        .map(|name| add_node(&mut project, NodeContainer::Clip(clip_id), solid_node(name)))
        .collect::<Result<Vec<_>>>()?;
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("merge"),
    )?;
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let ids = [Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)];
    use BlendMode::{LinearDodge, Multiply, Screen};
    let blends = [LinearDodge, Multiply, Screen];
    let mut connections = sources
        .into_iter()
        .zip(ids)
        .zip(blends)
        .map(|((source_id, id), blend_mode)| {
            let mut connection = ProjectConnection::new(
                address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                target.clone(),
                4,
            );
            connection.id = id;
            connection.blend_mode = blend_mode;
            connection
        })
        .collect::<Vec<_>>();
    connections.reverse();
    project.connections = connections;
    Ok((project, target, ids))
}

#[test]
fn reorder_duplicate_variadic_orders_uses_uuid_visible_order_and_preserves_wires() -> Result<()> {
    let (project, target, ids) = reverse_stored_duplicate_merge_project()?;
    let persisted = project.save()?;
    let mut project = Project::load(&persisted)?;
    let original_connections = project.connections.clone();
    assert_eq!(
        project
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>(),
        vec![ids[2], ids[1], ids[0]],
        "loading must preserve malformed storage order until an explicit edit",
    );
    assert!(project.validate_connections().contains(
        &ProjectGraphError::DuplicateConnectionOrder {
            target: target.clone(),
            order: 4,
        }
    ));

    // UUID order is the canonical visible tie-break, so moving the first row
    // one step toward Front produces middle, low, high despite reverse storage.
    project.reorder_connection(ids[0], 1)?;
    let mut canonical = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    canonical.sort_by_key(|connection| (connection.order, connection.id));
    assert_eq!(
        canonical
            .iter()
            .map(|connection| (connection.id, connection.order))
            .collect::<Vec<_>>(),
        vec![(ids[1], 0), (ids[0], 1), (ids[2], 2)],
    );
    assert!(project.validate_connections().is_empty());

    for connection in &project.connections {
        let original = original_connections
            .iter()
            .find(|original| original.id == connection.id)
            .with_context(|| format!("original connection {} must exist", connection.id))?;
        assert_eq!(connection.from, original.from);
        assert_eq!(connection.to, original.to);
        assert_eq!(connection.blend_mode, original.blend_mode);
    }
    Ok(())
}

#[test]
fn disconnect_normalizes_duplicate_variadic_orders_by_uuid_without_losing_blends() -> Result<()> {
    let (mut project, target, ids) = reverse_stored_duplicate_merge_project()?;
    let original_connections = project.connections.clone();

    assert_eq!(project.disconnect_connections([ids[1]]), 1);
    let mut canonical = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    canonical.sort_by_key(|connection| (connection.order, connection.id));
    assert_eq!(
        canonical
            .iter()
            .map(|connection| (connection.id, connection.order))
            .collect::<Vec<_>>(),
        vec![(ids[0], 0), (ids[2], 1)],
    );
    assert!(project.validate_connections().is_empty());

    for connection in canonical {
        let original = original_connections
            .iter()
            .find(|original| original.id == connection.id)
            .with_context(|| format!("original connection {} must exist", connection.id))?;
        assert_eq!(connection.from, original.from);
        assert_eq!(connection.to, original.to);
        assert_eq!(connection.blend_mode, original.blend_mode);
    }
    Ok(())
}
