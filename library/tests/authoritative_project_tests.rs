mod support;

use anyhow::{Context, Result, anyhow, bail};
use library::ProjectModel;
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::framing::get_frame_from_project;
use library::model::frame::entity::{FrameContent, FrameItem, FrameObject};
use library::model::project::{
    Composition, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    PortAddress, PortOwner, Project, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Clip, Node};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

use support::generator_node_for_canvas;

fn rewrite_persisted_node(
    node: &mut Node,
    update: impl FnOnce(&mut serde_json::Value),
) -> Result<()> {
    let mut encoded = serde_json::to_value(&*node).context("test Node must serialize")?;
    update(&mut encoded);

    *node = serde_json::from_value(encoded).context("mutated test Node must deserialize")?;
    Ok(())
}

fn insert_persisted_property(node: &mut Node, key: &str, property: Property) -> Result<()> {
    let encoded_property =
        serde_json::to_value(property).context("test Property must serialize")?;
    rewrite_persisted_node(node, |encoded| {
        encoded["properties"][key] = encoded_property;
    })
}

fn project_with_solid() -> Result<(Project, uuid::Uuid, uuid::Uuid)> {
    let mut project = Project::new("authoritative");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let clip = Clip::new("solid clip", 0.0, 2.0);
    let clip_id = clip.id;
    let mut node = generator_node_for_canvas(
        "solid",
        GeneratorNodeRequest::Solid {
            color: Default::default(),
        },
        320,
        180,
        320,
        180,
    );
    node.set_property(
        "position".to_string(),
        Property::constant(PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(10.0),
            y: OrderedFloat(20.0),
        })),
    )
    .map_err(|error| anyhow!(error))?;
    let node_id = node.id;

    project
        .add_track(track)
        .expect("container structural Merge insertion must succeed");
    project.add_clip(clip);
    project.add_node(node);
    project
        .add_composition(composition)
        .expect("container structural Merge insertion must succeed");
    project.attach_clip_to_track(track_id, clip_id)?;
    project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id)?;
    project.set_output_node(NodeContainer::Clip(clip_id), Some(node_id))?;
    Ok((project, composition_id, node_id))
}

fn rendered_position(project: &Project, plugin_manager: &Arc<PluginManager>) -> Result<(f64, f64)> {
    let frame = get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugin_manager.get_property_evaluators(),
        plugin_manager,
    )?;
    fn first_object(items: &[FrameItem]) -> Option<&FrameObject> {
        items.iter().find_map(|item| match item {
            FrameItem::Object(object) => Some(object),
            FrameItem::Group(group) => first_object(&group.items),
        })
    }

    let object = first_object(&frame.items).context("frame should contain the solid layer")?;
    let FrameContent::Shape { transform, .. } = &object.content else {
        bail!("solid generator should project to a shape frame object");
    };
    Ok((transform.position.x, transform.position.y))
}

fn read_project(project: &RwLock<Project>) -> Result<std::sync::RwLockReadGuard<'_, Project>> {
    project
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))
}

#[test]
fn load_and_undo_style_replacement_keep_every_consumer_on_the_same_arc() -> Result<()> {
    let (initial, _, _) = project_with_solid()?;
    let shared = Arc::new(RwLock::new(initial));
    let timeline_consumer = Arc::clone(&shared);
    let preview_consumer = Arc::clone(&shared);
    let plugin_manager = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(Arc::clone(&shared), plugin_manager);

    let mut loaded = Project::new("loaded");
    let (composition, track) = Composition::new("loaded composition", 640, 360, 24.0, 5.0);
    let loaded_composition_id = composition.id;
    loaded
        .add_track(track)
        .expect("container structural Merge insertion must succeed");
    loaded
        .add_composition(composition)
        .expect("container structural Merge insertion must succeed");
    manager.load_project(&loaded.save()?)?;

    assert!(Arc::ptr_eq(&shared, &timeline_consumer));
    assert!(Arc::ptr_eq(&shared, &preview_consumer));
    assert_eq!(read_project(&timeline_consumer)?.name, "loaded");
    assert_eq!(
        read_project(&preview_consumer)?
            .compositions
            .first()
            .context("loaded Composition must exist")?
            .id,
        loaded_composition_id
    );
    assert_eq!(Project::load(&manager.save_project()?)?, loaded);

    let (restored, _, _) = project_with_solid()?;
    manager.set_project(restored.clone())?;
    assert_eq!(*read_project(&timeline_consumer)?, restored);
    assert_eq!(*read_project(&preview_consumer)?, restored);
    Ok(())
}

#[test]
fn set_and_load_reject_invalid_structure_without_replacing_the_current_project() -> Result<()> {
    let (current, _, _) = project_with_solid()?;
    let shared = Arc::new(RwLock::new(current.clone()));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    let mut invalid = current.clone();
    invalid.compositions.push(
        invalid
            .compositions
            .first()
            .context("fixture Composition must exist")?
            .clone(),
    );

    assert!(matches!(
        manager.set_project(invalid.clone()),
        Err(library::LibraryError::Validation(_))
    ));
    assert_eq!(*read_project(&shared)?, current);

    assert!(matches!(
        manager.load_project(&invalid.save()?),
        Err(library::LibraryError::Validation(_))
    ));
    assert_eq!(*read_project(&shared)?, current);
    Ok(())
}

#[test]
fn adoption_preserves_sparse_pre_v1_generator_without_repair_or_rejection() -> Result<()> {
    let (candidate, _, node_id) = project_with_solid()?;
    let mut serialized_candidate = serde_json::to_value(candidate)?;
    serialized_candidate["nodes"][node_id.to_string()]["properties"] = serde_json::json!({});
    let candidate: Project = serde_json::from_value(serialized_candidate)?;

    let shared = Arc::new(RwLock::new(Project::new("current")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));

    manager.set_project(candidate.clone())?;
    assert_eq!(*read_project(&shared)?, candidate);

    let serialized = candidate.save()?;
    manager.load_project(&serialized)?;
    let project = read_project(&shared)?;
    assert_eq!(*project, candidate);
    let properties = project
        .get_node(node_id)
        .context("loaded sparse Generator is missing")?
        .properties();
    assert!(
        properties.iter().next().is_none(),
        "loading an incomplete pre-v1 Project must not synthesize properties"
    );
    Ok(())
}

#[test]
fn adoption_preserves_explicit_plugin_operation_nodes_unknown_to_this_binary() -> Result<()> {
    let (mut candidate, _, node_id) = project_with_solid()?;
    insert_persisted_property(
        candidate
            .get_node_mut(node_id)
            .context("solid fixture Node must exist")?,
        "future_plugin_property",
        Property::constant(PropertyValue::String("preserve me".to_string())),
    )?;
    let Some(NodeContainer::Clip(clip_id)) = candidate.find_node_container(node_id) else {
        bail!("solid fixture must live in a Clip");
    };
    let plugins = PluginManager::default();
    let mut effect = plugins.create_effect_operation_node("blur")?;
    let mut effector = plugins.create_effector_operation_node("transform")?;
    let mut decorator = plugins.create_decorator_operation_node("backplate")?;
    let mut style = plugins.create_style_operation_node("fill")?;
    for (node, unavailable_id) in [
        (&mut effect, "third_party.effect.not_installed"),
        (&mut effector, "third_party.effector.not_installed"),
        (&mut decorator, "third_party.decorator.not_installed"),
        (&mut style, "third_party.style.not_installed"),
    ] {
        let encoded_property = serde_json::to_value(Property::constant(PropertyValue::String(
            "preserve exactly".to_string(),
        )))?;
        rewrite_persisted_node(node, |encoded| {
            encoded["content"]["data"]["component_id"] =
                serde_json::Value::String(unavailable_id.to_string());
            encoded["properties"]["future_vendor_value"] = encoded_property;
        })?;
    }
    let shape = generator_node_for_canvas(
        "shape source",
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 100 V 100 Z".to_string(),
        },
        320,
        180,
        100,
        100,
    );
    let merge = Node::new_merge("result");
    let effect_id = effect.id;
    let shape_id = shape.id;
    let effector_id = effector.id;
    let decorator_id = decorator.id;
    let style_id = style.id;
    let merge_id = merge.id;
    for node in [effect, shape, effector, decorator, style, merge] {
        let id = node.id;
        candidate.add_node(node);
        candidate.attach_node_to_container(NodeContainer::Clip(clip_id), id)?;
    }
    for (from, to, order) in [
        (
            PortAddress::new(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
            0,
        ),
        (
            PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effector_id), SHAPE_INPUT_PORT),
            0,
        ),
        (
            PortAddress::new(PortOwner::Node(effector_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(decorator_id), SHAPE_INPUT_PORT),
            0,
        ),
        (
            PortAddress::new(PortOwner::Node(decorator_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
            0,
        ),
        (
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            0,
        ),
        (
            PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            1,
        ),
    ] {
        let connection_id = candidate.connect_ports(from, to)?;
        candidate.reorder_connection(connection_id, order)?;
    }
    candidate.set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))?;

    let shared = Arc::new(RwLock::new(Project::new("current")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    manager.set_project(candidate.clone())?;
    assert_eq!(*read_project(&shared)?, candidate);

    manager.load_project(&candidate.save()?)?;
    assert_eq!(*read_project(&shared)?, candidate);
    Ok(())
}

#[test]
fn legacy_embedded_operation_fields_are_rejected_instead_of_migrated() -> Result<()> {
    #[derive(Clone, Copy, Debug)]
    enum LegacyOwner {
        Node,
        Clip,
        Track,
        Composition,
    }

    let (candidate, _, node_id) = project_with_solid()?;
    let composition = candidate
        .compositions
        .first()
        .context("fixture Composition must exist")?;
    let track_id = *composition
        .track_ids
        .first()
        .context("fixture Track id must exist")?;
    let clip_id = *candidate
        .get_track(track_id)
        .context("fixture Track must exist")?
        .clip_ids
        .first()
        .context("fixture Clip id must exist")?;
    let current = Project::new("current project must survive rejected load");
    let shared = Arc::new(RwLock::new(current.clone()));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));

    let legacy_fields = [
        (LegacyOwner::Node, "styles"),
        (LegacyOwner::Node, "effects"),
        (LegacyOwner::Node, "effectors"),
        (LegacyOwner::Node, "decorators"),
        (LegacyOwner::Clip, "effects"),
        (LegacyOwner::Track, "effects"),
        (LegacyOwner::Composition, "effects"),
    ];

    for (owner, field) in legacy_fields {
        let mut json = serde_json::to_value(&candidate)?;
        let target = match owner {
            LegacyOwner::Node => json["nodes"]
                .get_mut(node_id.to_string())
                .context("serialized Node must exist")?,
            LegacyOwner::Clip => json["clips"]
                .get_mut(clip_id.to_string())
                .context("serialized Clip must exist")?,
            LegacyOwner::Track => json["tracks"]
                .get_mut(track_id.to_string())
                .context("serialized Track must exist")?,
            LegacyOwner::Composition => json["compositions"]
                .get_mut(0)
                .context("serialized Composition must exist")?,
        };
        target
            .as_object_mut()
            .context("serialized owner must be an object")?
            .insert(field.to_string(), serde_json::json!([]));
        let serialized = serde_json::to_string(&json)?;

        let error = match manager.load_project(&serialized) {
            Ok(_) => bail!("legacy {owner:?}.{field} unexpectedly loaded"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains(&format!("unknown field `{field}`")),
            "{owner:?}.{field} failed with the wrong error: {error}"
        );
        assert_eq!(
            *read_project(&shared)?,
            current,
            "rejected {owner:?}.{field} must not replace the current Project"
        );
    }
    Ok(())
}

#[test]
fn inspector_mutation_immediately_reaches_timeline_preview_save_and_export_snapshot() -> Result<()>
{
    let (project, composition_id, node_id) = project_with_solid()?;
    let shared = Arc::new(RwLock::new(project));
    let plugin_manager = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::clone(&plugin_manager));

    let initial_project = read_project(&shared)?;
    assert_eq!(
        rendered_position(&initial_project, &plugin_manager)?,
        (10.0, 20.0)
    );
    drop(initial_project);

    manager.update_property_or_keyframe(
        library::PropertyOwner::Node(node_id),
        "position",
        0.0,
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(42.0),
            y: OrderedFloat(84.0),
        }),
        None,
    )?;

    let project = read_project(&shared)?;
    let composition = project
        .get_composition(composition_id)
        .context("Composition must exist after mutation")?;
    let track_id = *composition
        .track_ids
        .first()
        .context("Composition Track id must exist")?;
    let track = project
        .get_track(track_id)
        .context("Track must exist after mutation")?;
    let clip_id = *track.clip_ids.first().context("Track Clip id must exist")?;
    let clip = project
        .get_clip(clip_id)
        .context("Clip must exist after mutation")?;
    assert_eq!(
        clip.node_ids,
        vec![node_id],
        "Timeline reads the same Project"
    );
    assert_eq!(
        rendered_position(&project, &plugin_manager)?,
        (42.0, 84.0),
        "Preview frame projection reflects the mutation without synchronization"
    );
    drop(project);

    let saved = manager.save_project()?;
    let saved_project = Project::load(&saved)?;
    assert_eq!(
        rendered_position(&saved_project, &plugin_manager)?,
        (42.0, 84.0),
        "save/load preserves the exact state observed by Preview"
    );

    // Export deliberately owns an immutable job snapshot, but that snapshot is
    // captured from the same latest authoritative Project rather than a second
    // editable model.
    let export_model = ProjectModel::new(Arc::new(saved_project), 0)?;
    assert_eq!(
        rendered_position(export_model.project(), &plugin_manager)?,
        (42.0, 84.0)
    );
    Ok(())
}
