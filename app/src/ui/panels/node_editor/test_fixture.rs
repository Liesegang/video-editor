use super::*;
use crate::test_support::generator_node;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::project::{IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, TIME_PORT};

pub(super) fn fixture() -> (Project, Uuid, Uuid, Uuid, Uuid, Uuid) {
    let mut project = Project::new("Node editor test");
    let (mut composition, mut track) =
        library::model::Composition::new("Main", 1920, 1080, 30.0, 10.0);
    composition.ui_position = [10.0, 20.0];
    composition.ui_size = [1400.0, 1000.0];
    track.ui_position = [110.0, 140.0];
    track.ui_size = [1100.0, 720.0];
    let composition_id = composition.id;
    let track_id = track.id;
    project
        .add_track(track)
        .expect("container structural Merge insertion must succeed");
    project
        .add_composition(composition)
        .expect("container structural Merge insertion must succeed");

    let mut clip = library::model::Clip::new("Clip", 1.0, 5.0);
    clip.ui_position = [260.0, 260.0];
    clip.ui_size = [760.0, 480.0];
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut solid = generator_node(
        "Solid",
        GeneratorNodeRequest::Solid {
            color: library::model::frame::color::Color {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
        },
    );
    solid.ui_position = [450.0, 390.0];
    let solid_id = solid.id;
    project.add_node(solid);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), solid_id)
        .unwrap();

    let mut merge = Node::new_merge("Merge");
    merge.ui_position = [770.0, 390.0];
    let merge_id = merge.id;
    project.add_node(merge);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), merge_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(solid_id), TIME_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(solid_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();

    (
        project,
        composition_id,
        track_id,
        clip_id,
        solid_id,
        merge_id,
    )
}
