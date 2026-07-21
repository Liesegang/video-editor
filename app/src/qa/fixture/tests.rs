use super::*;
use library::model::project::asset::AssetKind;
use library::model::project::{AUDIO_OUTPUT_PORT, MERGE_SOUNDS_PORT};

fn installed_fixture() -> (Arc<RwLock<Project>>, Arc<PluginManager>, FixtureInfo) {
    let project = Arc::new(RwLock::new(Project::new("empty")));
    let plugin_manager = Arc::new(PluginManager::default());
    let info = install_named(&project, NODE_EDITOR_E2E_FIXTURE, &plugin_manager).unwrap();
    (project, plugin_manager, info)
}

fn installed_transform_fixture() -> (Arc<RwLock<Project>>, Arc<PluginManager>, FixtureInfo) {
    let project = Arc::new(RwLock::new(Project::new("empty")));
    let plugin_manager = Arc::new(PluginManager::default());
    let info = install_named(&project, TRANSFORM_PREVIEW_E2E_FIXTURE, &plugin_manager).unwrap();
    (project, plugin_manager, info)
}

fn assert_connection(
    project: &Project,
    from_owner: PortOwner,
    from_port: &str,
    to_owner: PortOwner,
    to_port: &str,
    order: i64,
) {
    let matching = project
        .connections
        .iter()
        .filter(|connection| {
            connection.from == PortAddress::new(from_owner, from_port)
                && connection.to == PortAddress::new(to_owner, to_port)
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "missing or duplicate fixture wire");
    assert_eq!(matching[0].order, order);
}

fn assert_operation(
    project: &Project,
    plugin_manager: &PluginManager,
    node_id: Uuid,
    category: &str,
    component_id: &str,
) {
    let node = project.get_node(node_id).unwrap();
    let NodeContent::PluginOperation(operation) = node.content() else {
        panic!("{node_id} must be a PluginOperation Node");
    };
    assert_eq!(operation.category, category);
    assert_eq!(operation.component_id, component_id);
    let descriptor = plugin_manager
        .operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        )
        .unwrap();
    assert_eq!(
        operation.declared_ports.as_slice(),
        descriptor.declared_ports()
    );
    for definition in descriptor.properties() {
        assert!(
            node.properties().get(definition.name()).is_some(),
            "{} is missing {}",
            node.name,
            definition.name()
        );
    }
}

#[test]
fn fixture_uses_explicit_operation_nodes_and_output_bindings() {
    let (project, plugin_manager, info) = installed_fixture();
    let read = project.read().unwrap();
    assert_eq!(info.composition_id, E2E_COMPOSITION_ID);
    let composition = &read.compositions[0];
    assert_eq!(composition.track_ids, info.expanded_tracks);
    assert_eq!(
        composition.node_ids,
        vec![
            composition.structural_merge_node_id,
            composition.structural_sound_merge_node_id,
        ],
        "Composition owns one canonical Image Merge and one canonical Sound Merge"
    );
    assert_eq!(
        composition.output_node_id,
        Some(composition.structural_merge_node_id)
    );
    assert_eq!(
        composition.audio_output_node_id,
        Some(composition.structural_sound_merge_node_id)
    );
    assert_eq!(
        read.get_track(E2E_TRACK_A_ID).unwrap().clip_ids,
        vec![E2E_CLIP_A1_ID, E2E_CLIP_A2_ID]
    );
    assert_eq!(
        read.get_track(E2E_TRACK_B_ID).unwrap().clip_ids,
        vec![E2E_CLIP_B1_ID]
    );

    let clip_a1 = read.get_clip(E2E_CLIP_A1_ID).unwrap();
    assert_eq!(
        clip_a1.node_ids,
        vec![E2E_AUDIO_A_ID, E2E_AUDIO_B_ID, E2E_SOLID_ID, E2E_MERGE_ID,]
    );
    assert_eq!(clip_a1.output_node_id, Some(E2E_MERGE_ID));
    assert!(clip_a1.audio_output_node_id.is_none());
    let clip_a2 = read.get_clip(E2E_CLIP_A2_ID).unwrap();
    assert_eq!(
        clip_a2.node_ids,
        vec![
            E2E_AUX_A_ID,
            E2E_TEXT_TRANSFORM_ID,
            E2E_EFFECTOR_TRANSFORM_ID,
            E2E_EFFECTOR_OPACITY_ID,
            E2E_BACKPLATE_SHAPE_ID,
            E2E_DECORATOR_BACKPLATE_ID,
            E2E_TEXT_FILL_ID,
            E2E_BACKPLATE_FILL_ID,
            E2E_TEXT_MERGE_ID,
            E2E_BLUR_EFFECT_ID,
        ]
    );
    assert_eq!(clip_a2.output_node_id, Some(E2E_BLUR_EFFECT_ID));
    let clip_b1 = read.get_clip(E2E_CLIP_B1_ID).unwrap();
    assert_eq!(
        clip_b1.node_ids,
        vec![
            E2E_AUX_B_ID,
            E2E_SHAPE_TRANSFORM_ID,
            E2E_SHAPE_FILL_ID,
            E2E_SHAPE_STROKE_ID,
            E2E_SHAPE_MERGE_ID,
        ]
    );
    assert_eq!(clip_b1.output_node_id, Some(E2E_SHAPE_MERGE_ID));

    for (node_id, asset_id) in [
        (E2E_AUDIO_A_ID, E2E_AUDIO_ASSET_A_ID),
        (E2E_AUDIO_B_ID, E2E_AUDIO_ASSET_B_ID),
    ] {
        let NodeContent::Media(media) = read.get_node(node_id).unwrap().content() else {
            panic!("{node_id} must be a Media Node");
        };
        assert_eq!(media.asset_id, asset_id);
        assert_eq!(
            read.assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .unwrap()
                .kind,
            AssetKind::Audio
        );
    }

    let text = read.get_node(E2E_AUX_A_ID).unwrap();
    assert!(matches!(
        text.content(),
        NodeContent::Generator(library::model::GeneratorContent::Text)
    ));
    assert!(matches!(
        read.get_node(E2E_AUX_B_ID).unwrap().content(),
        NodeContent::Generator(library::model::GeneratorContent::Shape)
    ));
    for content_id in [E2E_AUX_A_ID, E2E_AUX_B_ID] {
        let content = read.get_node(content_id).unwrap();
        for property in ["position", "rotation", "scale", "anchor", "opacity"] {
            assert!(
                content.properties().get(property).is_none(),
                "{} must not duplicate {property} ownership",
                content.name
            );
        }
    }
    for transform_id in [E2E_TEXT_TRANSFORM_ID, E2E_SHAPE_TRANSFORM_ID] {
        let transform = read.get_node(transform_id).unwrap();
        for property in ["position", "rotation", "scale", "anchor"] {
            assert!(
                transform.properties().get(property).is_some(),
                "{} must own {property}",
                transform.name
            );
        }
    }
    for style_id in [
        E2E_TEXT_FILL_ID,
        E2E_BACKPLATE_FILL_ID,
        E2E_SHAPE_FILL_ID,
        E2E_SHAPE_STROKE_ID,
    ] {
        let style = read.get_node(style_id).unwrap();
        assert!(
            style.properties().get("opacity").is_some(),
            "{} must own opacity",
            style.name
        );
    }

    for (node_id, category, component_id) in [
        (E2E_TEXT_TRANSFORM_ID, "transform", "transform"),
        (E2E_SHAPE_TRANSFORM_ID, "transform", "transform"),
        (E2E_EFFECTOR_TRANSFORM_ID, "effector", "transform"),
        (E2E_EFFECTOR_OPACITY_ID, "effector", "opacity"),
        (E2E_DECORATOR_BACKPLATE_ID, "decorator", "backplate"),
        (E2E_BLUR_EFFECT_ID, "effect", "blur"),
        (E2E_TEXT_FILL_ID, "style", "fill"),
        (E2E_BACKPLATE_FILL_ID, "style", "fill"),
        (E2E_SHAPE_FILL_ID, "style", "fill"),
        (E2E_SHAPE_STROKE_ID, "style", "stroke"),
    ] {
        assert_operation(&read, &plugin_manager, node_id, category, component_id);
    }

    assert_eq!(
        read.nodes.len(),
        25,
        "19 authored Nodes plus the Image/Sound structural Merge pair for three containers"
    );
    for track in read.tracks.values() {
        assert_eq!(
            track.node_ids,
            vec![
                track.structural_merge_node_id,
                track.structural_sound_merge_node_id,
            ]
        );
        assert_eq!(track.output_node_id, Some(track.structural_merge_node_id));
        assert_eq!(
            track.audio_output_node_id,
            Some(track.structural_sound_merge_node_id)
        );
    }
    assert!(read.validate_connections().is_empty());
    assert!(read.validate_containment().is_empty());
    drop(read);
    assert!(install_named(&project, NODE_EDITOR_E2E_FIXTURE, &plugin_manager).is_err());
}

#[test]
fn fixture_wires_shape_and_image_flow_with_stable_merge_order() {
    let (project, _plugin_manager, _info) = installed_fixture();
    let read = project.read().unwrap();

    for (from_node, to_node) in [
        (E2E_AUX_A_ID, E2E_TEXT_TRANSFORM_ID),
        (E2E_TEXT_TRANSFORM_ID, E2E_EFFECTOR_TRANSFORM_ID),
        (E2E_EFFECTOR_TRANSFORM_ID, E2E_EFFECTOR_OPACITY_ID),
        (E2E_EFFECTOR_OPACITY_ID, E2E_DECORATOR_BACKPLATE_ID),
        (E2E_DECORATOR_BACKPLATE_ID, E2E_BACKPLATE_FILL_ID),
        (E2E_EFFECTOR_OPACITY_ID, E2E_TEXT_FILL_ID),
        (E2E_AUX_B_ID, E2E_SHAPE_TRANSFORM_ID),
        (E2E_SHAPE_TRANSFORM_ID, E2E_SHAPE_FILL_ID),
        (E2E_SHAPE_TRANSFORM_ID, E2E_SHAPE_STROKE_ID),
    ] {
        assert_connection(
            &read,
            PortOwner::Node(from_node),
            SHAPE_OUTPUT_PORT,
            PortOwner::Node(to_node),
            SHAPE_INPUT_PORT,
            0,
        );
    }
    assert_connection(
        &read,
        PortOwner::Node(E2E_BACKPLATE_SHAPE_ID),
        SHAPE_OUTPUT_PORT,
        PortOwner::Node(E2E_DECORATOR_BACKPLATE_ID),
        BACKGROUND_SHAPE_INPUT_PORT,
        0,
    );
    for (source_node, order) in [(E2E_BACKPLATE_FILL_ID, 0), (E2E_TEXT_FILL_ID, 1)] {
        assert_connection(
            &read,
            PortOwner::Node(source_node),
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(E2E_TEXT_MERGE_ID),
            MERGE_IMAGES_PORT,
            order,
        );
    }
    assert_connection(
        &read,
        PortOwner::Node(E2E_TEXT_MERGE_ID),
        IMAGE_OUTPUT_PORT,
        PortOwner::Node(E2E_BLUR_EFFECT_ID),
        IMAGE_INPUT_PORT,
        0,
    );
    for (source_node, order) in [(E2E_SHAPE_FILL_ID, 0), (E2E_SHAPE_STROKE_ID, 1)] {
        assert_connection(
            &read,
            PortOwner::Node(source_node),
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(E2E_SHAPE_MERGE_ID),
            MERGE_IMAGES_PORT,
            order,
        );
    }
    for (source_owner, order) in [
        (PortOwner::Node(E2E_SOLID_ID), 0),
        (PortOwner::Clip(E2E_CLIP_A2_ID), 1),
        (PortOwner::Clip(E2E_CLIP_B1_ID), 2),
    ] {
        assert_connection(
            &read,
            source_owner,
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(E2E_MERGE_ID),
            MERGE_IMAGES_PORT,
            order,
        );
    }

    for (clip_id, node_id) in [
        (E2E_CLIP_A1_ID, E2E_AUDIO_A_ID),
        (E2E_CLIP_A1_ID, E2E_AUDIO_B_ID),
        (E2E_CLIP_A1_ID, E2E_SOLID_ID),
        (E2E_CLIP_A2_ID, E2E_AUX_A_ID),
        (E2E_CLIP_A2_ID, E2E_TEXT_TRANSFORM_ID),
        (E2E_CLIP_A2_ID, E2E_EFFECTOR_TRANSFORM_ID),
        (E2E_CLIP_A2_ID, E2E_EFFECTOR_OPACITY_ID),
        (E2E_CLIP_A2_ID, E2E_BACKPLATE_SHAPE_ID),
        (E2E_CLIP_A2_ID, E2E_DECORATOR_BACKPLATE_ID),
        (E2E_CLIP_A2_ID, E2E_TEXT_FILL_ID),
        (E2E_CLIP_A2_ID, E2E_BACKPLATE_FILL_ID),
        (E2E_CLIP_A2_ID, E2E_BLUR_EFFECT_ID),
        (E2E_CLIP_B1_ID, E2E_AUX_B_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_TRANSFORM_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_FILL_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_STROKE_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_MERGE_ID),
    ] {
        assert_connection(
            &read,
            PortOwner::Clip(clip_id),
            TIME_PORT,
            PortOwner::Node(node_id),
            TIME_PORT,
            0,
        );
    }

    assert!(!read.connections.iter().any(|connection| {
        connection.to == PortAddress::new(PortOwner::Node(E2E_MERGE_ID), TIME_PORT)
    }));
    let track_a_merge = read
        .get_track(E2E_TRACK_A_ID)
        .unwrap()
        .structural_merge_node_id;
    let track_b_merge = read
        .get_track(E2E_TRACK_B_ID)
        .unwrap()
        .structural_merge_node_id;
    let composition_merge = read.compositions[0].structural_merge_node_id;
    for (source, target, order) in [
        (PortOwner::Clip(E2E_CLIP_A1_ID), track_a_merge, 0),
        (PortOwner::Clip(E2E_CLIP_A2_ID), track_a_merge, 1),
        (PortOwner::Clip(E2E_CLIP_B1_ID), track_b_merge, 0),
        (PortOwner::Track(E2E_TRACK_A_ID), composition_merge, 0),
        (PortOwner::Track(E2E_TRACK_B_ID), composition_merge, 1),
    ] {
        assert_connection(
            &read,
            source,
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(target),
            MERGE_IMAGES_PORT,
            order,
        );
    }
    let track_a_sound_merge = read
        .get_track(E2E_TRACK_A_ID)
        .unwrap()
        .structural_sound_merge_node_id;
    let track_b_sound_merge = read
        .get_track(E2E_TRACK_B_ID)
        .unwrap()
        .structural_sound_merge_node_id;
    let composition_sound_merge = read.compositions[0].structural_sound_merge_node_id;
    for (source, target, order) in [
        (PortOwner::Clip(E2E_CLIP_A1_ID), track_a_sound_merge, 0),
        (PortOwner::Clip(E2E_CLIP_A2_ID), track_a_sound_merge, 1),
        (PortOwner::Clip(E2E_CLIP_B1_ID), track_b_sound_merge, 0),
        (PortOwner::Track(E2E_TRACK_A_ID), composition_sound_merge, 0),
        (PortOwner::Track(E2E_TRACK_B_ID), composition_sound_merge, 1),
    ] {
        assert_connection(
            &read,
            source,
            AUDIO_OUTPUT_PORT,
            PortOwner::Node(target),
            MERGE_SOUNDS_PORT,
            order,
        );
    }
    assert_eq!(read.connections.len(), 45);
    assert!(read.validate_connections().is_empty());
}

#[test]
fn transform_preview_fixture_has_two_independent_clip_spatial_roots() {
    let (project, plugin_manager, _info) = installed_transform_fixture();
    let read = project.read().unwrap();
    assert_eq!(
        read.get_track(E2E_TRACK_B_ID).unwrap().clip_ids,
        vec![E2E_CLIP_B1_ID, E2E_AMBIGUOUS_CLIP_ID]
    );
    let clip = read.get_clip(E2E_AMBIGUOUS_CLIP_ID).unwrap();
    assert_eq!(
        clip.node_ids,
        vec![
            E2E_AMBIGUOUS_SHAPE_A_ID,
            E2E_AMBIGUOUS_TRANSFORM_A_ID,
            E2E_AMBIGUOUS_FILL_A_ID,
            E2E_AMBIGUOUS_SHAPE_B_ID,
            E2E_AMBIGUOUS_TRANSFORM_B_ID,
            E2E_AMBIGUOUS_FILL_B_ID,
            E2E_AMBIGUOUS_MERGE_ID,
        ]
    );
    assert_eq!(clip.output_node_id, Some(E2E_AMBIGUOUS_MERGE_ID));
    for transform_id in [E2E_AMBIGUOUS_TRANSFORM_A_ID, E2E_AMBIGUOUS_TRANSFORM_B_ID] {
        assert_operation(
            &read,
            &plugin_manager,
            transform_id,
            "transform",
            "transform",
        );
    }
    for (shape, transform, fill) in [
        (
            E2E_AMBIGUOUS_SHAPE_A_ID,
            E2E_AMBIGUOUS_TRANSFORM_A_ID,
            E2E_AMBIGUOUS_FILL_A_ID,
        ),
        (
            E2E_AMBIGUOUS_SHAPE_B_ID,
            E2E_AMBIGUOUS_TRANSFORM_B_ID,
            E2E_AMBIGUOUS_FILL_B_ID,
        ),
    ] {
        assert_connection(
            &read,
            PortOwner::Node(shape),
            SHAPE_OUTPUT_PORT,
            PortOwner::Node(transform),
            SHAPE_INPUT_PORT,
            0,
        );
        assert_connection(
            &read,
            PortOwner::Node(transform),
            SHAPE_OUTPUT_PORT,
            PortOwner::Node(fill),
            SHAPE_INPUT_PORT,
            0,
        );
        assert_connection(
            &read,
            PortOwner::Node(fill),
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(E2E_AMBIGUOUS_MERGE_ID),
            MERGE_IMAGES_PORT,
            if fill == E2E_AMBIGUOUS_FILL_A_ID {
                0
            } else {
                1
            },
        );
    }
    assert_connection(
        &read,
        PortOwner::Clip(E2E_AMBIGUOUS_CLIP_ID),
        IMAGE_OUTPUT_PORT,
        PortOwner::Node(E2E_MERGE_ID),
        MERGE_IMAGES_PORT,
        3,
    );
    assert!(read.validate_connections().is_empty());
    assert!(read.validate_containment().is_empty());
}
