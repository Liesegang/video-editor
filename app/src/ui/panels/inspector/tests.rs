use super::*;
use crate::test_support::generator_node;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::frame::color::Color;
use library::model::project::NodeContainer;
use library::model::property::Property;
use library::plugin::{
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, PATH_EFFECT_CATEGORY,
    SHAPE_TRANSFORM_COMPONENT_ID, TRANSFORM_APPLY_OPERATION,
};

#[test]
fn clip_selection_keeps_the_clip_as_its_semantic_identity() {
    let mut project = Project::new("inspector");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let first = Node::new_merge("first");
    let second = Node::new_merge("second");
    let mut clip = Clip::new("clip", 2.0, 4.0);
    let clip_id = clip.id;
    let first_id = first.id;
    let second_id = second.id;
    clip.node_ids = vec![second_id, first_id];
    clip.output_node_id = Some(first_id);
    project.add_node(first);
    project.add_node(second);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let Some(InspectorSelection::Clip { clip, track_id }) = resolve_selection(
        &project,
        Some(SelectionTarget::Clip(clip_id)),
        composition_id,
    ) else {
        panic!("Clip selection should resolve");
    };
    assert_eq!(clip.id, clip_id);
    assert_eq!(clip.node_ids, vec![second_id, first_id]);
    assert_eq!(track_id, project.find_track_for_clip(clip_id));
}

#[test]
fn direct_node_selection_stays_node_owned() {
    let mut project = Project::new("inspector");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let node = Node::new_merge("leaf");
    let node_id = node.id;
    let clip = Clip::new("clip", 3.0, 5.0);
    let clip_id = clip.id;
    project.add_node(node);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();

    let Some(InspectorSelection::Node {
        node,
        containing_clip,
        ..
    }) = resolve_selection(
        &project,
        Some(SelectionTarget::Node(node_id)),
        composition_id,
    )
    else {
        panic!("Node selection should resolve");
    };
    assert_eq!(node.id, node_id);
    assert_eq!(containing_clip.unwrap().id, clip_id);
}

#[test]
fn same_uuid_node_and_clip_resolve_by_explicit_target_kind() {
    let mut project = Project::new("same UUID inspector");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let shared_id = Uuid::new_v4();
    let mut clip = Clip::new("clip with shared UUID", 0.0, 5.0);
    clip.id = shared_id;
    let mut node = Node::new_merge("node with shared UUID");
    node.id = shared_id;

    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.add_clip(clip);
    project.add_node(node);
    project.attach_clip_to_track(track_id, shared_id).unwrap();
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), shared_id)
        .unwrap();

    let Some(InspectorSelection::Clip { clip, .. }) = resolve_selection(
        &project,
        Some(SelectionTarget::Clip(shared_id)),
        composition_id,
    ) else {
        panic!("typed Clip target should resolve the Clip registry");
    };
    assert_eq!(clip.name, "clip with shared UUID");

    let Some(InspectorSelection::Node { node, track_id, .. }) = resolve_selection(
        &project,
        Some(SelectionTarget::Node(shared_id)),
        composition_id,
    ) else {
        panic!("typed Node target should resolve the Node registry");
    };
    assert_eq!(node.name, "node with shared UUID");
    assert_eq!(track_id, None);
}

#[test]
fn timeline_track_and_composition_resolve_without_a_leaf_selection() {
    let mut project = Project::new("timeline owner inspector");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 5.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let Some(InspectorSelection::Track { track, .. }) = resolve_selection(
        &project,
        Some(SelectionTarget::Track(track_id)),
        composition_id,
    ) else {
        panic!("Track selection should resolve");
    };
    assert_eq!(track.id, track_id);

    let Some(InspectorSelection::Composition { composition, .. }) =
        resolve_selection(&project, None, composition_id)
    else {
        panic!("Composition selection should resolve");
    };
    assert_eq!(composition.id, composition_id);
}

#[test]
fn explicit_selection_from_another_composition_does_not_fall_back() {
    let mut project = Project::new("composition scoped inspector");
    let (active, active_track) = Composition::new("active", 1920, 1080, 30.0, 5.0);
    let active_id = active.id;
    assert!(
        project.add_track(active_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(active).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let (other, other_track) = Composition::new("other", 1920, 1080, 30.0, 5.0);
    let other_track_id = other_track.id;
    let mut other_clip = Clip::new("other clip", 0.0, 1.0);
    let other_clip_id = other_clip.id;
    let other_node = Node::new_merge("other node");
    let other_node_id = other_node.id;
    other_clip.node_ids.push(other_node_id);
    assert!(
        project.add_track(other_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(other).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.add_node(other_node);
    project.add_clip(other_clip);
    project
        .attach_clip_to_track(other_track_id, other_clip_id)
        .unwrap();

    assert!(resolve_selection(
        &project,
        Some(SelectionTarget::Node(other_node_id)),
        active_id,
    )
    .is_none());
}

#[test]
fn structural_status_reuses_the_authoritative_clip_semantics(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut project = Project::new("inspector graph semantics");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let clip = Clip::new("clip", 0.0, 10.0);
    let clip_id = clip.id;
    let source = generator_node(
        "Title",
        GeneratorNodeRequest::Text {
            text: "Title".to_string(),
            font: "Arial".to_string(),
        },
    );
    let plugin_manager = PluginManager::default();
    let applied = plugin_manager.create_style_operation_node("fill")?;
    let disconnected = plugin_manager.create_effect_operation_node("blur")?;
    let result = Node::new_merge("Composite");
    let source_id = source.id;
    let applied_id = applied.id;
    let disconnected_id = disconnected.id;
    let result_id = result.id;

    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    for node in [source, applied, disconnected, result] {
        let node_id = node.id;
        project.add_node(node);
        project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id)?;
    }
    project.connect_ports(
        library::model::project::PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
        library::model::project::PortAddress::new(PortOwner::Node(applied_id), SHAPE_INPUT_PORT),
    )?;
    let result_connection_id = project.connect_ports(
        library::model::project::PortAddress::new(PortOwner::Node(applied_id), IMAGE_OUTPUT_PORT),
        library::model::project::PortAddress::new(PortOwner::Node(result_id), MERGE_IMAGES_PORT),
    )?;
    project.set_output_node(NodeContainer::Clip(clip_id), Some(result_id))?;
    let Some(result_connection) = project
        .connections
        .iter_mut()
        .find(|connection| connection.id == result_connection_id)
    else {
        return Err(std::io::Error::other("result connection was not retained").into());
    };
    result_connection.order = 3;
    project.connections.push(ProjectConnection::new(
        library::model::project::PortAddress::new(PortOwner::Node(applied_id), "property.opacity"),
        library::model::project::PortAddress::new(
            PortOwner::Node(disconnected_id),
            "property.sigma_x",
        ),
        99,
    ));

    let Some(InspectorSelection::Clip { clip, .. }) = resolve_selection(
        &project,
        Some(SelectionTarget::Clip(clip_id)),
        composition_id,
    ) else {
        return Err(std::io::Error::other("Clip selection should resolve").into());
    };
    assert_eq!(clip.id, clip_id);
    let semantics = project.container_graph_semantics(PortOwner::Clip(clip_id));
    let connections = connections_for_nodes(&project, &clip.node_ids);
    assert_eq!(
        semantics,
        project.container_graph_semantics(PortOwner::Clip(clip_id))
    );
    assert!(semantics.structurally_reaches_output(PortOwner::Node(source_id)));
    assert!(semantics.structurally_reaches_output(PortOwner::Node(applied_id)));
    assert!(semantics.structurally_reaches_output(PortOwner::Node(result_id)));
    assert!(!semantics.structurally_reaches_output(PortOwner::Node(disconnected_id)));
    let outgoing = connections
        .iter()
        .filter(|connection| {
            connection.from.owner == PortOwner::Node(applied_id)
                && is_content_flow_connection(connection)
        })
        .collect::<Vec<_>>();
    assert_eq!(outgoing.len(), 1, "scalar wires are not semantic branches");
    let metadata = content_connection_metadata(outgoing[0]);
    assert_eq!(metadata["connection_id"], serde_json::json!(outgoing[0].id));
    assert_eq!(
        metadata["from_owner"],
        serde_json::json!(PortOwner::Node(applied_id))
    );
    assert_eq!(metadata["from_port"], IMAGE_OUTPUT_PORT);
    assert_eq!(
        metadata["to_owner"],
        serde_json::json!(PortOwner::Node(result_id))
    );
    assert_eq!(metadata["to_port"], MERGE_IMAGES_PORT);
    assert_eq!(metadata["order"], 3);
    assert_eq!(
        operation_state_label(true, true, false, false, true, &outgoing),
        "Wired to result · order 3"
    );
    assert_eq!(
        operation_state_label(true, true, false, false, false, &[]),
        "Not wired to result"
    );
    assert_eq!(
        operation_state_label(false, true, false, false, true, &outgoing),
        "Unavailable"
    );
    assert_eq!(
        operation_state_label(true, false, true, false, true, &outgoing),
        "Disabled"
    );
    assert_eq!(
        operation_state_label(false, true, true, false, true, &outgoing),
        "Bypassed"
    );
    Ok(())
}

#[test]
fn facade_output_mode_describes_timeline_child_compositing() {
    let result = Node::new_merge("Composite");
    let nodes = [result.clone()];

    for owner_kind in [FacadeOwnerKind::Composition, FacadeOwnerKind::Track] {
        let output_mode = owner_kind.output_mode(Some(result.id));
        assert_eq!(
            output_mode,
            FacadeOutputMode::TimelineChildren(Some(result.id))
        );
        assert_eq!(output_mode.qa_value(), "timeline_children");
        assert!(facade_output_text(owner_kind, output_mode, &nodes)
            .contains("structural Merge and authored downstream graph"));
        let metadata = facade_output_metadata(owner_kind, output_mode, true);
        assert_eq!(metadata["owner_kind"], owner_kind.qa_value());
        assert_eq!(metadata["output_mode"], "timeline_children");
        assert_eq!(metadata["output_node_id"], serde_json::json!(result.id));
        assert_eq!(metadata["explicit"], true);
    }

    let composition_mode = FacadeOwnerKind::Composition.output_mode(None);
    assert_eq!(composition_mode, FacadeOutputMode::TimelineChildren(None));
    assert_eq!(composition_mode.qa_value(), "timeline_children");
    assert_eq!(
        facade_output_text(FacadeOwnerKind::Composition, composition_mode, &nodes,),
        "Composes ordered child Tracks through the structural Merge (NoOutput: no result binding)"
    );
    let composition_metadata =
        facade_output_metadata(FacadeOwnerKind::Composition, composition_mode, false);
    assert_eq!(composition_metadata["output_mode"], "timeline_children");
    assert_eq!(
        composition_metadata["output_node_id"],
        serde_json::Value::Null
    );
    assert_eq!(composition_metadata["explicit"], false);

    let track_mode = FacadeOwnerKind::Track.output_mode(None);
    assert_eq!(track_mode, FacadeOutputMode::TimelineChildren(None));
    assert_eq!(track_mode.qa_value(), "timeline_children");
    assert_eq!(
        facade_output_text(FacadeOwnerKind::Track, track_mode, &nodes),
        "Composes ordered child Clips through the structural Merge (NoOutput: no result binding)"
    );
    let track_metadata = facade_output_metadata(FacadeOwnerKind::Track, track_mode, false);
    assert_eq!(track_metadata["owner_kind"], "track");
    assert_eq!(track_metadata["output_mode"], "timeline_children");
}

#[test]
fn inferred_definitions_cover_editable_values_and_skip_structures() {
    let mut properties = PropertyMap::new();
    properties.set(
        "gain".into(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    );
    properties.set(
        "display_name".into(),
        Property::constant(PropertyValue::String("Title".into())),
    );
    properties.set(
        "metadata".into(),
        Property::constant(PropertyValue::Map(Default::default())),
    );

    let definitions = inferred_property_definitions(&properties, 0.0);
    assert_eq!(
        definitions
            .iter()
            .map(|definition| definition.name())
            .collect::<Vec<_>>(),
        vec!["display_name", "gain"]
    );
    assert_eq!(property_label("display_name"), "Display Name");
}

#[test]
fn installed_plugin_operation_uses_authoritative_inspector_ranges() {
    let plugins = PluginManager::default();
    let node = plugins.create_style_operation_node("stroke").unwrap();
    let definitions = plugin_operation_property_definitions(&plugins, &node)
        .expect("installed operation descriptor");
    let width = definitions
        .iter()
        .find(|definition| definition.name() == "width")
        .expect("Stroke width definition");
    assert!(matches!(
        width.ui_type(),
        PropertyUiType::Float {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            suffix,
            min_hard_limit: false,
            max_hard_limit: false,
        } if suffix == "px"
    ));
    assert_eq!(width.default_value(), &PropertyValue::from(1.0));
    let join = definitions
        .iter()
        .find(|definition| definition.name() == "join")
        .expect("Stroke join definition");
    assert!(matches!(
        join.ui_type(),
        PropertyUiType::Dropdown { options }
            if options == &["Miter".to_string(), "Round".to_string(), "Bevel".to_string()]
    ));
    assert_eq!(
        join.default_value(),
        &PropertyValue::String("Round".to_string())
    );

    let inferred = inferred_property_definitions(node.properties(), 0.0);
    let inferred_width = inferred
        .iter()
        .find(|definition| definition.name() == "width")
        .unwrap();
    assert!(matches!(
        inferred_width.ui_type(),
        PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            ..
        }
    ));
    assert_ne!(width.ui_type(), inferred_width.ui_type());
}

#[test]
fn fmod_uses_canonical_divisor_metadata_instead_of_inferred_ranges() {
    let node = Node::new_fmod("Fmod");
    let definitions = canonical_value_property_definitions(&node).unwrap();
    let divisor = definitions
        .iter()
        .find(|definition| definition.name() == "divisor")
        .unwrap();
    assert_eq!(divisor.label(), "Divisor");
    assert_eq!(divisor.default_value(), &PropertyValue::from(1.0));
    assert!(matches!(
        divisor.ui_type(),
        PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            step: 0.01,
            suffix,
            min_hard_limit: false,
            max_hard_limit: false,
        } if suffix.is_empty()
    ));

    let inferred = inferred_property_definitions(node.properties(), 0.0);
    assert_ne!(inferred[0].ui_type(), divisor.ui_type());
}

#[test]
fn root_transform_has_transform_semantics_and_descriptor_property_controls() {
    let plugins = PluginManager::default();
    let node = plugins.create_shape_transform_operation_node().unwrap();
    let NodeContent::PluginOperation(operation) = node.content() else {
        panic!("Transform factory returned a PluginOperation")
    };
    assert_eq!(operation.category, TRANSFORM_CATEGORY);
    assert_eq!(operation.component_id, SHAPE_TRANSFORM_COMPONENT_ID);
    assert_eq!(operation.operation, TRANSFORM_APPLY_OPERATION);
    assert_eq!(operation_category(&node), Some(TRANSFORM_CATEGORY));
    assert_eq!(source_kind(&node), "Transform");
    assert_eq!(node_display_type(&node), "Transform");
    assert!(OPERATION_CATEGORY_SECTIONS.contains(&(
        TRANSFORM_CATEGORY,
        "Transform",
        "Root placement"
    )));

    let definitions = plugin_operation_property_definitions(&plugins, &node)
        .expect("installed Transform descriptor drives generic Inspector controls");
    assert_eq!(
        definitions
            .iter()
            .map(PropertyDefinition::name)
            .collect::<Vec<_>>(),
        vec!["position", "rotation", "scale", "anchor"]
    );
    assert_eq!(definitions.len(), node.properties().iter().count());
    for definition in &definitions {
        assert_eq!(
            node.properties()
                .get(definition.name())
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(definition.default_value().clone())
        );
    }
    let position = definitions
        .iter()
        .find(|definition| definition.name() == "position")
        .unwrap();
    assert!(matches!(
        position.ui_type(),
        PropertyUiType::Vec2 { suffix, .. } if suffix == "px"
    ));
    let rotation = definitions
        .iter()
        .find(|definition| definition.name() == "rotation")
        .unwrap();
    assert!(matches!(
        rotation.ui_type(),
        PropertyUiType::Float {
            min: -360.0,
            max: 360.0,
            step: 1.0,
            suffix,
            min_hard_limit: false,
            max_hard_limit: false,
        } if suffix == "deg"
    ));
}

#[test]
fn value_nodes_are_numeric_operations_and_never_visual_sources() {
    let source = generator_node(
        "Solid",
        GeneratorNodeRequest::Solid {
            color: Color::black(),
        },
    );
    let source_id = source.id;
    let value = Node::new_fmod("Fmod");
    let value_id = value.id;
    let merge = Node::new_merge("Merge");
    let nodes = vec![source, value, merge];

    assert_eq!(
        semantic_visual_sources(&nodes)
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![source_id]
    );
    assert_eq!(
        native_value_nodes(&nodes)
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![value_id]
    );
    assert_eq!(source_kind(&nodes[1]), "Fmod");
}

#[test]
fn effect_operation_descriptor_drives_inspector_and_qa_metadata() {
    let plugins = PluginManager::default();
    let node = plugins.create_effect_operation_node("blur").unwrap();
    let definitions = plugin_operation_property_definitions(&plugins, &node)
        .expect("installed Effect descriptor");
    let sigma_x = definitions
        .iter()
        .find(|definition| definition.name() == "sigma_x")
        .expect("Blur sigma_x definition");
    assert_eq!(
        properties::property_definition_metadata(sigma_x),
        serde_json::json!({
            "name": "sigma_x",
            "label": "Sigma X",
            "default": 0.0,
            "ui": {
                "kind": "float",
                "min": 0.0,
                "max": 100.0,
                "step": 0.1,
                "suffix": "px",
                "min_hard_limit": true,
                "max_hard_limit": false,
            },
        })
    );
    let tile_mode = definitions
        .iter()
        .find(|definition| definition.name() == "tile_mode")
        .expect("Blur tile_mode definition");
    assert_eq!(
        properties::property_definition_metadata(tile_mode),
        serde_json::json!({
            "name": "tile_mode",
            "label": "Tile Mode",
            "default": "clamp",
            "ui": {
                "kind": "dropdown",
                "options": ["clamp", "repeat", "mirror", "decal"],
            },
        })
    );
}

#[test]
fn effector_descriptor_initializes_and_describes_transform_and_opacity_controls() {
    let plugins = PluginManager::default();
    for component_id in ["transform", "opacity"] {
        let node = plugins
            .create_effector_operation_node(component_id)
            .unwrap();
        let definitions = plugin_operation_property_definitions(&plugins, &node)
            .expect("installed Effector descriptor");
        assert_eq!(definitions.len(), node.properties().iter().count());
        for definition in &definitions {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(|property| property.evaluate_at(0.0).ok()),
                Some(definition.default_value().clone()),
                "{component_id}.{} must be initialized by its descriptor factory",
                definition.name(),
            );
        }
        let target = definitions
            .iter()
            .find(|definition| definition.name() == "target")
            .expect("Effector target definition");
        assert_eq!(
            properties::property_definition_metadata(target),
            serde_json::json!({
                "name": "target",
                "label": "Target",
                "default": "Block",
                "ui": {
                    "kind": "dropdown",
                    "options": ["Block", "Line", "Char"],
                },
            })
        );
    }

    let opacity = plugins.create_effector_operation_node("opacity").unwrap();
    let definitions = plugin_operation_property_definitions(&plugins, &opacity).unwrap();
    let mode = definitions
        .iter()
        .find(|definition| definition.name() == "mode")
        .expect("Opacity mode definition");
    assert_eq!(
        properties::property_definition_metadata(mode),
        serde_json::json!({
            "name": "mode",
            "label": "Mode",
            "default": "Set",
            "ui": {
                "kind": "dropdown",
                "options": ["Set", "Add", "Multiply"],
            },
        })
    );
}

#[test]
fn unknown_plugin_operation_roundtrips_and_falls_back_to_lossless_generic_controls() {
    let plugins = PluginManager::default();
    let node = plugins.create_effector_operation_node("opacity").unwrap();
    let node_id = node.id;
    let NodeContent::PluginOperation(operation) = node.content() else {
        panic!("factory returned a PluginOperation")
    };
    let expected_ports = operation.declared_ports.clone();
    let mut encoded_node = serde_json::to_value(node).unwrap();
    let operation = encoded_node["content"]["data"]
        .as_object_mut()
        .expect("serialized PluginOperation content");
    operation.insert(
        "category".to_string(),
        serde_json::Value::String(EFFECTOR_CATEGORY.to_string()),
    );
    operation.insert(
        "component_id".to_string(),
        serde_json::Value::String("third.party.unavailable-opacity".to_string()),
    );
    operation.insert(
        "operation".to_string(),
        serde_json::Value::String(EFFECTOR_APPLY_OPERATION.to_string()),
    );
    let node: Node = serde_json::from_value(encoded_node).unwrap();
    let expected_node = node.clone();

    let mut project = Project::new("foreign plugin roundtrip");
    project.add_node(node);
    let encoded = serde_json::to_value(&project).unwrap();
    let decoded: Project = serde_json::from_value(encoded).unwrap();
    let restored = decoded
        .get_node(node_id)
        .expect("roundtripped operation Node");
    assert_eq!(restored, &expected_node);
    let NodeContent::PluginOperation(restored_operation) = restored.content() else {
        panic!("roundtripped PluginOperation identity")
    };
    assert_eq!(restored_operation.declared_ports, expected_ports);
    assert!(plugin_operation_property_definitions(&plugins, restored).is_none());
    assert_eq!(
        node_display_type(restored),
        format!(
            "Plugin Operation · {} / {}",
            EFFECTOR_CATEGORY, EFFECTOR_APPLY_OPERATION
        )
    );
    let fallback = inferred_property_definitions(restored.properties(), 0.0);
    assert_eq!(fallback.len(), restored.properties().iter().count());
    for property_name in ["opacity", "mode", "target"] {
        assert!(
            fallback
                .iter()
                .any(|definition| definition.name() == property_name),
            "unknown plugin value {property_name} must remain generically inspectable"
        );
    }
}

#[test]
fn path_effect_inspector_metadata_is_exact_and_path_only() {
    assert!(OPERATION_CATEGORY_SECTIONS.contains(&(
        PATH_EFFECT_CATEGORY,
        "Path Effects",
        "Path geometry only"
    )));
    let plugins = PluginManager::default();
    let discrete = plugins
        .create_path_effect_operation_node("discrete")
        .unwrap();
    assert_eq!(source_kind(&discrete), "Path Effect");
    assert_eq!(
        node_display_type(&discrete),
        "Path Effect · Path geometry only"
    );
    let definitions = plugin_operation_property_definitions(&plugins, &discrete).unwrap();
    let metadata = definitions
        .iter()
        .map(properties::property_definition_metadata)
        .collect::<Vec<_>>();
    assert_eq!(
        metadata,
        [
            serde_json::json!({
                "name": "segment_length",
                "label": "Segment Length",
                "default": 8.0,
                "ui": {
                    "kind": "float",
                    "min": 0.1,
                    "max": 1000.0,
                    "step": 1.0,
                    "suffix": "px",
                    "min_hard_limit": true,
                    "max_hard_limit": false,
                },
            }),
            serde_json::json!({
                "name": "deviation",
                "label": "Deviation",
                "default": 2.0,
                "ui": {
                    "kind": "float",
                    "min": 0.0,
                    "max": 1000.0,
                    "step": 1.0,
                    "suffix": "px",
                    "min_hard_limit": true,
                    "max_hard_limit": false,
                },
            }),
            serde_json::json!({
                "name": "seed",
                "label": "Seed",
                "default": 0,
                "ui": {
                    "kind": "integer",
                    "min": 0,
                    "max": i64::MAX,
                    "suffix": "",
                    "min_hard_limit": true,
                    "max_hard_limit": true,
                },
            }),
        ]
    );
}

#[test]
fn node_and_inspector_timing_adapters_derive_from_the_same_clip_metadata() {
    let duration = Clip::timing_property_definition("duration").unwrap();
    let node = crate::ui::panels::node_editor::node_timing_drag_config(duration).unwrap();
    let inspector = inspector_timing_drag_config(duration, 30.0, 120.0).unwrap();

    assert_eq!(inspector.speed, node.speed * 30.0);
    assert_eq!(
        inspector.hard_min,
        node.hard_min.map(|min| min * 30.0 + 120.0)
    );
    assert_eq!(
        inspector.hard_max,
        node.hard_max.map(|max| max * 30.0 + 120.0)
    );

    let stretch = Clip::timing_property_definition("time_stretch").unwrap();
    let node_stretch = crate::ui::panels::node_editor::node_timing_drag_config(stretch).unwrap();
    assert_eq!(node_stretch.hard_min, Some(0.0));
    assert!(stretch
        .validate_value(&PropertyValue::Number(OrderedFloat(0.0)))
        .is_ok());
}
