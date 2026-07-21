use super::*;

#[test]
fn malformed_foreign_image_binding_is_no_output_after_deserialization() {
    let mut project = Project::new("malformed foreign image binding");
    let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let first_clip = Clip::new("First", 0.0, 1.0);
    let first_clip_id = first_clip.id;
    project.add_clip(first_clip);
    project
        .attach_clip_to_track(track_id, first_clip_id)
        .unwrap();

    let second_clip = Clip::new("Second", 1.0, 1.0);
    let second_clip_id = second_clip.id;
    project.add_clip(second_clip);
    project
        .attach_clip_to_track(track_id, second_clip_id)
        .unwrap();

    let foreign_node = Node::new_merge("Foreign Image");
    let foreign_node_id = foreign_node.id;
    project.add_node(foreign_node);
    project
        .attach_node_to_container(NodeContainer::Clip(second_clip_id), foreign_node_id)
        .unwrap();

    let mut persisted = serde_json::to_value(&project).unwrap();
    let first = persisted["clips"]
        .as_object_mut()
        .unwrap()
        .get_mut(&first_clip_id.to_string())
        .unwrap();
    first["output_node_id"] = serde_json::json!(foreign_node_id);

    let malformed: Project = serde_json::from_value(persisted).unwrap();
    assert_eq!(
        malformed.find_node_container(foreign_node_id),
        Some(NodeContainer::Clip(second_clip_id))
    );
    assert!(
        malformed
            .container_image_sources(PortOwner::Clip(first_clip_id))
            .is_empty()
    );
}
