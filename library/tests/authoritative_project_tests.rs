use library::ProjectModel;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::entity::{FrameContent, FrameItem, FrameObject};
use library::model::project::{
    Composition, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    PortAddress, PortOwner, Project, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Clip, GeneratorContent, Node, NodeContent};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

fn project_with_solid() -> (Project, uuid::Uuid, uuid::Uuid) {
    let mut project = Project::new("authoritative");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let clip = Clip::new("solid clip", 0.0, 2.0);
    let clip_id = clip.id;
    let mut node = Node::new("solid", NodeContent::Generator(GeneratorContent::Solid));
    node.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Default::default())),
    );
    node.properties.set(
        "position".to_string(),
        Property::constant(PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(10.0),
            y: OrderedFloat(20.0),
        })),
    );
    let node_id = node.id;

    project.add_track(track);
    project.add_clip(clip);
    project.add_node(node);
    project.add_composition(composition);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();
    (project, composition_id, node_id)
}

fn rendered_position(project: &Project, plugin_manager: &Arc<PluginManager>) -> (f64, f64) {
    let frame = get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugin_manager.get_property_evaluators(),
        plugin_manager,
    )
    .unwrap();
    fn first_object(items: &[FrameItem]) -> Option<&FrameObject> {
        items.iter().find_map(|item| match item {
            FrameItem::Object(object) => Some(object),
            FrameItem::Group(group) => first_object(&group.items),
        })
    }

    let object = first_object(&frame.items).expect("frame should contain the solid layer");
    let FrameContent::Shape { transform, .. } = &object.content else {
        panic!("solid generator should project to a shape frame object");
    };
    (transform.position.x, transform.position.y)
}

#[test]
fn load_and_undo_style_replacement_keep_every_consumer_on_the_same_arc() {
    let (initial, _, _) = project_with_solid();
    let shared = Arc::new(RwLock::new(initial));
    let timeline_consumer = Arc::clone(&shared);
    let preview_consumer = Arc::clone(&shared);
    let plugin_manager = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(Arc::clone(&shared), plugin_manager);

    let mut loaded = Project::new("loaded");
    let (composition, track) = Composition::new("loaded composition", 640, 360, 24.0, 5.0);
    let loaded_composition_id = composition.id;
    loaded.add_track(track);
    loaded.add_composition(composition);
    manager.load_project(&loaded.save().unwrap()).unwrap();

    assert!(Arc::ptr_eq(&shared, &timeline_consumer));
    assert!(Arc::ptr_eq(&shared, &preview_consumer));
    assert_eq!(timeline_consumer.read().unwrap().name, "loaded");
    assert_eq!(
        preview_consumer.read().unwrap().compositions[0].id,
        loaded_composition_id
    );
    assert_eq!(
        Project::load(&manager.save_project().unwrap()).unwrap(),
        loaded
    );

    let (restored, _, _) = project_with_solid();
    manager.set_project(restored.clone()).unwrap();
    assert_eq!(*timeline_consumer.read().unwrap(), restored);
    assert_eq!(*preview_consumer.read().unwrap(), restored);
}

#[test]
fn set_and_load_reject_invalid_structure_without_replacing_the_current_project() {
    let (current, _, _) = project_with_solid();
    let shared = Arc::new(RwLock::new(current.clone()));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    let mut invalid = current.clone();
    invalid.compositions.push(invalid.compositions[0].clone());

    assert!(matches!(
        manager.set_project(invalid.clone()),
        Err(library::LibraryError::Validation(_))
    ));
    assert_eq!(*shared.read().unwrap(), current);

    assert!(matches!(
        manager.load_project(&invalid.save().unwrap()),
        Err(library::LibraryError::Validation(_))
    ));
    assert_eq!(*shared.read().unwrap(), current);
}

#[test]
fn adoption_preserves_explicit_plugin_operation_nodes_unknown_to_this_binary() {
    let (mut candidate, _, node_id) = project_with_solid();
    candidate.get_node_mut(node_id).unwrap().properties.set(
        "future_plugin_property".to_string(),
        Property::constant(PropertyValue::String("preserve me".to_string())),
    );
    let NodeContainer::Clip(clip_id) = candidate.find_node_container(node_id).unwrap() else {
        panic!("solid fixture must live in a Clip")
    };
    let plugins = PluginManager::default();
    let mut effect = plugins.create_effect_operation_node("blur").unwrap();
    let mut effector = plugins.create_effector_operation_node("transform").unwrap();
    let mut decorator = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let mut style = plugins.create_style_operation_node("fill").unwrap();
    for (node, unavailable_id) in [
        (&mut effect, "third_party.effect.not_installed"),
        (&mut effector, "third_party.effector.not_installed"),
        (&mut decorator, "third_party.decorator.not_installed"),
        (&mut style, "third_party.style.not_installed"),
    ] {
        let NodeContent::PluginOperation(operation) = &mut node.content else {
            panic!("plugin factory must return an operation Node")
        };
        operation.component_id = unavailable_id.to_string();
        node.properties.set(
            "future_vendor_value".to_string(),
            Property::constant(PropertyValue::String("preserve exactly".to_string())),
        );
    }
    let shape = Node::new(
        "shape source",
        NodeContent::Generator(GeneratorContent::Shape),
    );
    let merge = Node::new("result", NodeContent::Merge);
    let effect_id = effect.id;
    let shape_id = shape.id;
    let effector_id = effector.id;
    let decorator_id = decorator.id;
    let style_id = style.id;
    let merge_id = merge.id;
    for node in [effect, shape, effector, decorator, style, merge] {
        let id = node.id;
        candidate.add_node(node);
        candidate
            .attach_node_to_container(NodeContainer::Clip(clip_id), id)
            .unwrap();
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
        let connection_id = candidate.connect_ports(from, to).unwrap();
        candidate.reorder_connection(connection_id, order).unwrap();
    }
    candidate
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();

    let shared = Arc::new(RwLock::new(Project::new("current")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    manager.set_project(candidate.clone()).unwrap();
    assert_eq!(*shared.read().unwrap(), candidate);

    manager.load_project(&candidate.save().unwrap()).unwrap();
    assert_eq!(*shared.read().unwrap(), candidate);
}

#[test]
fn legacy_embedded_operation_fields_are_rejected_instead_of_migrated() {
    let (project, _, node_id) = project_with_solid();
    let mut json = serde_json::to_value(project).unwrap();
    json["nodes"][node_id.to_string()]["effects"] = serde_json::json!([]);
    let error = Project::load(&serde_json::to_string(&json).unwrap()).unwrap_err();
    assert!(error.to_string().contains("unknown field `effects`"));
}

#[test]
fn inspector_mutation_immediately_reaches_timeline_preview_save_and_export_snapshot() {
    let (project, composition_id, node_id) = project_with_solid();
    let shared = Arc::new(RwLock::new(project));
    let plugin_manager = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::clone(&plugin_manager));

    assert_eq!(
        rendered_position(&shared.read().unwrap(), &plugin_manager),
        (10.0, 20.0)
    );

    manager
        .update_property_or_keyframe(
            library::editor::handlers::property_ops::PropertyOwner::Node(node_id),
            "position",
            0.0,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(42.0),
                y: OrderedFloat(84.0),
            }),
            None,
        )
        .unwrap();

    let project = shared.read().unwrap();
    let composition = project.get_composition(composition_id).unwrap();
    let track = project.get_track(composition.track_ids[0]).unwrap();
    let clip = project.get_clip(track.clip_ids[0]).unwrap();
    assert_eq!(
        clip.node_ids,
        vec![node_id],
        "Timeline reads the same Project"
    );
    assert_eq!(
        rendered_position(&project, &plugin_manager),
        (42.0, 84.0),
        "Preview frame projection reflects the mutation without synchronization"
    );
    drop(project);

    let saved = manager.save_project().unwrap();
    let saved_project = Project::load(&saved).unwrap();
    assert_eq!(
        rendered_position(&saved_project, &plugin_manager),
        (42.0, 84.0),
        "save/load preserves the exact state observed by Preview"
    );

    // Export deliberately owns an immutable job snapshot, but that snapshot is
    // captured from the same latest authoritative Project rather than a second
    // editable model.
    let export_model = ProjectModel::new(Arc::new(saved_project), 0).unwrap();
    assert_eq!(
        rendered_position(export_model.project(), &plugin_manager),
        (42.0, 84.0)
    );
}
