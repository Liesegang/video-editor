use std::sync::{Arc, RwLock, RwLockReadGuard};

use anyhow::{Context, Result};
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::model::frame::color::Color;
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle,
    PortAddress, PortOwner, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Clip, Composition, NodeContent, Project};
use library::plugin::{
    IMAGE_OPACITY_STYLE_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID, PluginManager,
    SHAPE_TRANSFORM_COMPONENT_ID,
};
use ordered_float::OrderedFloat;
use uuid::Uuid;

fn read(project: &RwLock<Project>) -> Result<RwLockReadGuard<'_, Project>> {
    project
        .read()
        .map_err(|error| anyhow::anyhow!("Project read lock poisoned: {error}"))
}

fn project_with_clip(
    graph: NodeGraphBundle,
    plugins: Arc<PluginManager>,
) -> Result<(Arc<RwLock<Project>>, ProjectManager, Uuid)> {
    let mut project = Project::new("transform ensure");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("clip", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    Ok((shared, manager, clip_id))
}

fn component_nodes(project: &Project, clip_id: Uuid, component: &str) -> Vec<Uuid> {
    project
        .get_clip(clip_id)
        .map(|clip| clip.node_ids.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|node_id| {
            let NodeContent::PluginOperation(operation) = project.get_node(*node_id)?.content()
            else {
                return None;
            };
            (operation.component_id == component).then_some(*node_id)
        })
        .collect()
}

#[test]
fn ensure_shape_transform_before_rasterization_is_atomic_and_idempotent() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let shape = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 40 V 20 Z".to_string(),
        },
        320,
        180,
        40,
        20,
    )?;
    let shape_id = shape.id;
    let graph = NodeGraphBundle::new(vec![shape], Vec::new(), None);
    let (shared, manager, clip_id) = project_with_clip(graph, plugins)?;
    {
        let mut project = shared
            .write()
            .map_err(|error| anyhow::anyhow!("Project write lock poisoned: {error}"))?;
        let clip = project.get_clip_mut(clip_id).context("Clip exists")?;
        clip.properties.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(12.0),
                y: OrderedFloat(18.0),
            })),
        );
        clip.properties.set(
            "opacity".to_string(),
            Property::constant(PropertyValue::from(45.0)),
        );
    }
    let transform_id = manager.ensure_semantic_container_transform(NodeContainer::Clip(clip_id))?;
    let first = read(&shared)?.clone();
    assert_eq!(
        component_nodes(&first, clip_id, SHAPE_TRANSFORM_COMPONENT_ID),
        vec![transform_id]
    );
    assert!(component_nodes(&first, clip_id, IMAGE_OPACITY_STYLE_COMPONENT_ID).is_empty());
    assert_eq!(
        first
            .get_clip(clip_id)
            .context("Clip exists")?
            .output_node_id,
        None
    );
    assert!(first.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT)
    }));
    let clip = first.get_clip(clip_id).context("Clip exists")?;
    assert!(clip.properties.get("position").is_none());
    assert_eq!(
        clip.properties.get("opacity").and_then(Property::value),
        Some(&PropertyValue::from(45.0)),
        "Transform ensure must not materialize or absorb Image Opacity"
    );

    assert_eq!(
        manager.ensure_semantic_container_transform(NodeContainer::Clip(clip_id))?,
        transform_id
    );
    assert_eq!(*read(&shared)?, first);
    Ok(())
}

#[test]
fn ensure_raster_transform_does_not_implicitly_add_opacity() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let source = factory.create_generator_node(
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
        320,
        180,
        320,
        180,
    )?;
    let source_id = source.id;
    let graph = NodeGraphBundle::new(vec![source], Vec::new(), Some(source_id));
    let (shared, manager, clip_id) = project_with_clip(graph, plugins)?;
    let transform_id = manager.ensure_semantic_container_transform(NodeContainer::Clip(clip_id))?;
    let project = read(&shared)?;
    assert_eq!(
        component_nodes(&project, clip_id, IMAGE_TRANSFORM_COMPONENT_ID),
        vec![transform_id]
    );
    assert!(component_nodes(&project, clip_id, IMAGE_OPACITY_STYLE_COMPONENT_ID).is_empty());
    assert_eq!(
        project
            .get_clip(clip_id)
            .context("Clip exists")?
            .output_node_id,
        Some(transform_id)
    );
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT)
    }));
    Ok(())
}

#[test]
fn ensure_reuses_factory_shape_transform_without_rewriting_graph() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let graph = factory.create_text_graph("Title", "Arial", 320, 180)?;
    let (shared, manager, clip_id) = project_with_clip(graph, plugins)?;
    let before = read(&shared)?.clone();
    let expected = *component_nodes(&before, clip_id, SHAPE_TRANSFORM_COMPONENT_ID)
        .first()
        .context("factory Shape Transform")?;
    assert_eq!(
        manager.ensure_semantic_container_transform(NodeContainer::Clip(clip_id))?,
        expected
    );
    assert_eq!(*read(&shared)?, before);
    Ok(())
}

#[test]
fn ensure_transform_is_independent_of_ambiguous_opacity_branches() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let source = factory.create_generator_node(
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
        320,
        180,
        320,
        180,
    )?;
    let transform = plugins.create_image_transform_operation_node()?;
    let first_opacity = plugins.create_image_opacity_style_operation_node()?;
    let second_opacity = plugins.create_image_opacity_style_operation_node()?;
    let merge = library::model::Node::new_merge("opacity branches");
    let (source_id, transform_id, first_opacity_id, second_opacity_id, merge_id) = (
        source.id,
        transform.id,
        first_opacity.id,
        second_opacity.id,
        merge.id,
    );
    let graph = NodeGraphBundle::new(
        vec![source, transform, first_opacity, second_opacity, merge],
        vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(first_opacity_id), IMAGE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(second_opacity_id), IMAGE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(first_opacity_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(second_opacity_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                1,
            ),
        ],
        Some(merge_id),
    );
    let (shared, manager, clip_id) = project_with_clip(graph, plugins)?;
    let before = read(&shared)?.clone();

    assert_eq!(
        manager.ensure_semantic_container_transform(NodeContainer::Clip(clip_id))?,
        transform_id,
        "a unique authored Transform remains independently addressable"
    );
    assert_eq!(*read(&shared)?, before);
    Ok(())
}
