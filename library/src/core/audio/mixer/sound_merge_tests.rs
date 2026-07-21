use super::tests::{
    TestAudioFiles, add_audio_node, attach_audio_output, connect_attached_audio_output,
};
use super::*;
use crate::model::project::ProjectGraphError;
use crate::model::property::{Property, PropertyValue};
use crate::model::{Clip, NodeContainer};
use ordered_float::OrderedFloat;

#[test]
fn bypass_routes_first_ordered_input_and_disable_is_no_output() {
    let mut project = Project::new("Sound Merge state");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let sound_merge_id = track.structural_sound_merge_node_id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    for (name, sample) in [("first", 0.25), ("second", 0.5)] {
        let clip = Clip::new(name, 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        let node_id = add_audio_node(&mut project, &cache, &mut files, vec![sample; 4]);
        attach_audio_output(&mut project, NodeContainer::Clip(clip_id), node_id);
    }
    let mix = |project: &Project| {
        mix_samples(
            &project.assets,
            project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        )
    };
    assert_eq!(mix(&project), vec![0.75; 4]);

    assert!(project.get_node(sound_merge_id).unwrap().supports_bypass());
    project.get_node_mut(sound_merge_id).unwrap().bypassed = true;
    assert_eq!(
        mix(&project),
        vec![0.25; 4],
        "bypass must retain only the first canonical Sound input"
    );

    project.get_node_mut(sound_merge_id).unwrap().enabled = false;
    assert_eq!(
        mix(&project),
        vec![0.0; 4],
        "disabled Sound Merge must be NoOutput rather than implicit passthrough"
    );
}

#[test]
fn parent_sources_join_children_and_direct_bindings_fail_transactionally() {
    let mut project = Project::new("audio canonical parent nodes");
    let (composition, mut track) = Composition::new("main", 1920, 1080, 30.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    track.properties.set(
        "volume".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    );
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let composition_node = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 4]);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), composition_node)
        .unwrap();
    let track_node = add_audio_node(&mut project, &cache, &mut files, vec![2.0; 4]);
    project
        .attach_node_to_container(NodeContainer::Track(track_id), track_node)
        .unwrap();
    let clip = Clip::new("audio clip", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let clip_node = add_audio_node(&mut project, &cache, &mut files, vec![4.0; 4]);
    attach_audio_output(&mut project, NodeContainer::Clip(clip_id), clip_node);

    let mix = |project: &Project| {
        mix_samples(
            &project.assets,
            project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        )
    };
    assert_eq!(mix(&project), vec![2.0; 4]);

    let before_invalid_binding = project.clone();
    assert!(matches!(
        project.set_audio_output_node(NodeContainer::Track(track_id), Some(track_node)),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput { .. })
    ));
    assert_eq!(project, before_invalid_binding);
    connect_attached_audio_output(&mut project, NodeContainer::Track(track_id), track_node);
    assert_eq!(mix(&project), vec![3.0; 4]);

    let before_invalid_binding = project.clone();
    assert!(matches!(
        project.set_audio_output_node(
            NodeContainer::Composition(composition_id),
            Some(composition_node),
        ),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput { .. })
    ));
    assert_eq!(project, before_invalid_binding);
    connect_attached_audio_output(
        &mut project,
        NodeContainer::Composition(composition_id),
        composition_node,
    );
    assert_eq!(mix(&project), vec![4.0; 4]);
}
