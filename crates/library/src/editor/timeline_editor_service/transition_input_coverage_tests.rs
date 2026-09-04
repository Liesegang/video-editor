use super::*;

use crate::model::authoring::{
    InstanceLocator, ItemOutputStage, MediaInputBinding, MediaOutputKind, ModuleDefinitionSharing,
    ModulePortAddress, ModuleTemplateOrigin, PublishedMediaInput, PublishedMediaInputId, SourceRef,
    TransitionAlignment, TransitionMediaType, TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::node::Node;
use crate::model::project::{MERGE_IMAGES_PORT, PortDataType};

fn seconds(value: i64) -> MediaTime {
    MediaTime::from_whole_seconds(value)
}

struct CoverageFixture {
    service: TimelineEditorService,
    transition_id: TransitionId,
    definition_id: ModuleDefinitionId,
    input_id: PublishedMediaInputId,
    source_item_id: TimelineItemId,
    source_track_id: TimelineTrackId,
}

fn coverage_fixture(source_interval: TimelineInterval) -> CoverageFixture {
    let service = TimelineEditorService::create_default("Transition input coverage").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    let transition_track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);

    let solid = || SourceRef::Solid {
        color: Color::white(),
    };
    let (from_item_id, _) = service
        .add_item(
            transition_track_id,
            "From".to_string(),
            solid(),
            TimelineInterval::new(seconds(0), seconds(7)).unwrap(),
            0,
        )
        .unwrap();
    let (to_item_id, _) = service
        .add_item(
            transition_track_id,
            "To".to_string(),
            solid(),
            TimelineInterval::new(seconds(3), seconds(7)).unwrap(),
            1,
        )
        .unwrap();
    let (transition_id, _) = service
        .add_transition(TransitionPlacement {
            from_item_id,
            to_item_id,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        })
        .unwrap();

    let source_track_id = service
        .add_track(
            timeline_id,
            "Auxiliary source".to_string(),
            TimelineTrackKind::Visual,
        )
        .unwrap()
        .0;
    let (source_item_id, _) = service
        .add_item(
            source_track_id,
            "Matte".to_string(),
            solid(),
            source_interval,
            0,
        )
        .unwrap();

    let (mut definition, _) = ModuleDefinition::new_transition(
        "Required matte",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let target = Node::new_merge("Matte input");
    let input_id = PublishedMediaInputId::new();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: input_id,
        name: "Matte".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id: target.id,
            port: MERGE_IMAGES_PORT.to_string(),
        },
        required: true,
        primary: false,
    });
    definition.graph.nodes.insert(target.id, target);
    definition.topology_revision += 1;
    definition.interface_version += 1;
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();

    CoverageFixture {
        service,
        transition_id,
        definition_id,
        input_id,
        source_item_id,
        source_track_id,
    }
}

fn required_binding(fixture: &CoverageFixture) -> MediaInputBinding {
    MediaInputBinding::TimelineItemOutput {
        locator: InstanceLocator::SameTimeline,
        item_id: fixture.source_item_id,
        output: MediaOutputKind::Image,
        stage: ItemOutputStage::PostTransform,
    }
}

fn assign(fixture: &CoverageFixture) -> Result<(ModuleInstanceId, ChangeSet), LibraryError> {
    fixture.service.assign_transition_module_with_controls(
        fixture.transition_id,
        fixture.definition_id,
        HashMap::from([(fixture.input_id, required_binding(fixture))]),
        HashMap::new(),
    )
}

#[test]
fn required_transition_input_rejects_an_off_time_source_atomically() {
    let fixture = coverage_fixture(TimelineInterval::new(seconds(0), seconds(2)).unwrap());
    let before = fixture.service.snapshot().unwrap();

    let error = assign(&fixture).expect_err("off-time source must be rejected");

    assert!(
        error.to_string().contains("full Transition interval"),
        "{error}"
    );
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        before.as_ref()
    );
}

#[test]
fn required_transition_input_rejects_partial_coverage_atomically() {
    let fixture = coverage_fixture(TimelineInterval::new(seconds(3), seconds(3)).unwrap());
    let before = fixture.service.snapshot().unwrap();

    let error = assign(&fixture).expect_err("partial source coverage must be rejected");

    assert!(
        error.to_string().contains("full Transition interval"),
        "{error}"
    );
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        before.as_ref()
    );
}

#[test]
fn required_transition_input_accepts_exact_boundary_and_superset_coverage() {
    for interval in [
        TimelineInterval::new(seconds(3), seconds(4)).unwrap(),
        TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
    ] {
        let fixture = coverage_fixture(interval);
        assign(&fixture).expect("complete source coverage must be accepted");
        fixture.service.snapshot().unwrap().validate().unwrap();
    }
}

#[test]
fn required_transition_input_requires_a_same_timeline_locator() {
    let fixture = coverage_fixture(TimelineInterval::new(seconds(0), seconds(10)).unwrap());
    let project = fixture.service.snapshot().unwrap();
    let root_path = InstancePath::root(project.root_timeline_id);
    drop(project);
    let binding = MediaInputBinding::TimelineItemOutput {
        locator: InstanceLocator::Exact(root_path),
        item_id: fixture.source_item_id,
        output: MediaOutputKind::Image,
        stage: ItemOutputStage::PostTransform,
    };

    let error = fixture
        .service
        .assign_transition_module_with_controls(
            fixture.transition_id,
            fixture.definition_id,
            HashMap::from([(fixture.input_id, binding)]),
            HashMap::new(),
        )
        .expect_err("required Exact binding has no single host-Timeline coverage clock");

    assert!(
        error.to_string().contains("must use SameTimeline"),
        "{error}"
    );
}

#[test]
fn moving_a_bound_required_source_out_of_coverage_is_rejected() {
    let fixture = coverage_fixture(TimelineInterval::new(seconds(0), seconds(10)).unwrap());
    assign(&fixture).unwrap();
    let before = fixture.service.snapshot().unwrap();

    let error = fixture
        .service
        .move_item(
            fixture.source_item_id,
            fixture.source_track_id,
            seconds(4),
            0,
        )
        .expect_err("placement edit must preserve required Transition coverage");

    assert!(
        error.to_string().contains("full Transition interval"),
        "{error}"
    );
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        before.as_ref()
    );
}
