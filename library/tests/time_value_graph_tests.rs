mod support;

use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;

use library::animation::EasingFunction;
use library::editor::project_service::MediaNodeRequest;
use library::framing::get_frame_from_project;
use library::model::asset::{Asset, AssetKind};
use library::model::frame::entity::{FrameContent, FrameGroup, FrameItem, FrameObject};
use library::model::project::{
    Composition, EvalOutput, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, FPS_PORT, FRAME_PORT,
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NodeContainer, PortAddress, PortDataType, PortDirection, PortExposure, PortOwner, Project,
    ProjectGraphError, TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue};
use library::model::{Clip, Node, NodeContent, ValueContent};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use uuid::Uuid;

use support::media_node_for_canvas;

const FPS: f64 = 10.0;

struct TimeGraphFixture {
    project: Project,
    clip_id: Uuid,
    fmod_id: Uuid,
    media_id: Uuid,
}

fn time_graph_fixture(
    start_time: f64,
    trim_in: f64,
    time_stretch: f64,
    wire_value: bool,
) -> Result<TimeGraphFixture> {
    let mut project = Project::new("time graph");
    let (composition, track) = Composition::new("main", 320, 180, FPS, 20.0);
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);

    let mut clip = Clip::new("video", start_time, 10.0);
    clip.trim_in = OrderedFloat(trim_in);
    clip.time_stretch = OrderedFloat(time_stretch);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;

    let mut asset = Asset::new("virtual", "virtual.mp4", AssetKind::Video);
    asset.duration = Some(100.0);
    let asset_id = asset.id;
    project.assets.push(asset);

    let fmod = Node::new_fmod("Fmod");
    let fmod_id = fmod.id;
    let media = media_node_for_canvas(
        "Video",
        MediaNodeRequest::Video {
            asset_id,
            file_path: "virtual.mp4".to_string(),
            stream_index: None,
            audio_stream_index: None,
        },
        320,
        180,
        320,
        180,
    );
    let media_id = media.id;
    for node in [fmod, media] {
        let id = node.id;
        project.add_node(node);
        project.attach_node_to_container(NodeContainer::Clip(clip_id), id)?;
    }
    if wire_value {
        project.connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
        )?;
    }
    project.connect_ports(
        PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(media_id), TIME_PORT),
    )?;
    project.set_output_node(NodeContainer::Clip(clip_id), Some(media_id))?;
    assert!(project.validate_connections().is_empty());

    Ok(TimeGraphFixture {
        project,
        clip_id,
        fmod_id,
        media_id,
    })
}

fn evaluate(
    project: &Project,
    frame_number: u64,
) -> Result<library::model::frame::frame::FrameInfo> {
    let plugins = Arc::new(PluginManager::default());
    Ok(get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?)
}

fn first_video_object(items: &[FrameItem]) -> Option<&FrameObject> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) if matches!(object.content, FrameContent::Video { .. }) => {
            Some(object)
        }
        FrameItem::Object(_) => None,
        FrameItem::Group(group) => first_video_object(&group.items),
    })
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

fn video_time(frame: &library::model::frame::frame::FrameInfo) -> Option<f64> {
    let object = first_video_object(&frame.items)?;
    let FrameContent::Video { source_time, .. } = object.content else {
        return None;
    };
    Some(source_time)
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1.0e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn direct_clip_output_applies_its_node_time_remap() -> Result<()> {
    let fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;

    // This direct output binding used to receive the Clip scope (2.5)
    // directly and therefore bypass the Media Node's explicit Time input.
    assert_close(
        video_time(&evaluate(&fixture.project, 25)?).context("frame 25 must contain Video time")?,
        0.5,
    );
    assert_close(
        video_time(&evaluate(&fixture.project, 35)?).context("frame 35 must contain Video time")?,
        0.5,
    );
    Ok(())
}

#[test]
fn operation_and_merge_paths_use_the_same_source_node_time_remap() -> Result<()> {
    let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    let plugins = PluginManager::default();
    let mut effect = plugins.create_effect_operation_node("blur")?;
    effect
        .set_property(
            "sigma_x".to_string(),
            Property::keyframe(vec![
                Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
                Keyframe::new(1.0, 10.0.into(), EasingFunction::Linear),
            ]),
        )
        .map_err(|error| anyhow!(error))?;
    let effect_id = effect.id;
    let merge = Node::new_merge("Merge");
    let merge_id = merge.id;
    for node in [effect, merge] {
        let id = node.id;
        fixture.project.add_node(node);
        fixture
            .project
            .attach_node_to_container(NodeContainer::Clip(fixture.clip_id), id)?;
    }
    fixture.project.connect_ports(
        PortAddress::new(PortOwner::Node(fixture.media_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
    )?;
    fixture.project.connect_ports(
        PortAddress::new(PortOwner::Node(fixture.fmod_id), NUMBER_RESULT_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(effect_id), TIME_PORT),
    )?;
    fixture.project.connect_ports(
        PortAddress::new(PortOwner::Node(effect_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    fixture
        .project
        .set_output_node(NodeContainer::Clip(fixture.clip_id), Some(merge_id))?;
    assert!(fixture.project.validate_connections().is_empty());

    let frame = evaluate(&fixture.project, 25)?;
    assert_close(
        video_time(&frame).context("effect frame must contain Video time")?,
        0.5,
    );
    let effect_group = find_group(&frame.items, effect_id).context("effect group must exist")?;
    assert_close(effect_group.effect_time.into_inner(), 0.5);
    assert_close(
        effect_group.effects[0].properties["sigma_x"]
            .get_as::<f64>()
            .context("sigma_x must be numeric")?,
        5.0,
    );
    Ok(())
}

#[test]
fn scalar_result_can_drive_a_number_property_and_a_time_input() -> Result<()> {
    let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    fixture.project.connect_ports(
        PortAddress::new(PortOwner::Node(fixture.fmod_id), NUMBER_RESULT_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(fixture.media_id), "opacity"),
    )?;

    let frame = evaluate(&fixture.project, 15)?;
    let object = first_video_object(&frame.items).context("Video object must exist")?;
    assert_close(
        video_time(&frame).context("scalar frame must contain Video time")?,
        0.5,
    );
    assert_close(object.source_transform.opacity, 0.005);
    Ok(())
}

#[test]
fn a_divisor_wire_overrides_the_authored_divisor_property() -> Result<()> {
    let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    fixture.project.connect_ports(
        PortAddress::new(PortOwner::Clip(fixture.clip_id), FPS_PORT),
        PortAddress::new(PortOwner::Node(fixture.fmod_id), FMOD_DIVISOR_INPUT_PORT),
    )?;

    // FPS=10 overrides the authored divisor=1, so 2.5 remains 2.5.
    assert_close(
        video_time(&evaluate(&fixture.project, 25)?)
            .context("divisor override frame must contain Video time")?,
        2.5,
    );
    Ok(())
}

#[test]
fn missing_invalid_and_disabled_fmod_inputs_produce_no_output() -> Result<()> {
    let mut cases = Vec::new();

    cases.push(("missing value", time_graph_fixture(0.0, 0.0, 1.0, false)?));
    for (label, divisor) in [
        ("zero divisor", 0.0),
        ("negative zero divisor", -0.0),
        ("non-finite divisor", f64::NAN),
    ] {
        let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
        fixture
            .project
            .get_node_mut(fixture.fmod_id)
            .context("Fmod Node must exist")?
            .set_property(
                FMOD_DIVISOR_INPUT_PORT.to_string(),
                Property::constant(PropertyValue::Number(OrderedFloat(divisor))),
            )
            .map_err(|error| anyhow!(error))?;
        cases.push((label, fixture));
    }
    let mut disabled = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    disabled
        .project
        .get_node_mut(disabled.fmod_id)
        .context("disabled Fmod Node must exist")?
        .enabled = false;
    cases.push(("disabled", disabled));

    for (label, fixture) in cases {
        assert!(
            video_time(&evaluate(&fixture.project, 5)?).is_none(),
            "{label} must propagate NoOutput"
        );
    }
    Ok(())
}

#[test]
fn missing_authored_divisor_produces_no_output_instead_of_a_default() -> Result<()> {
    let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    let node = fixture
        .project
        .get_node(fixture.fmod_id)
        .context("Fmod Node must exist")?;
    let mut json = serde_json::to_value(node)?;
    json["properties"]
        .as_object_mut()
        .context("serialized properties must be an object")?
        .remove(FMOD_DIVISOR_INPUT_PORT);
    let without_divisor: Node = serde_json::from_value(json)?;
    *fixture
        .project
        .get_node_mut(fixture.fmod_id)
        .context("Fmod Node must remain mutable")? = without_divisor;

    assert!(video_time(&evaluate(&fixture.project, 5)?).is_none());
    Ok(())
}

#[test]
fn expression_without_typed_fallback_propagates_no_output() -> Result<()> {
    let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    let malformed_expression = Property {
        evaluator: "expression".to_string(),
        properties: HashMap::from([(
            "expression".to_string(),
            PropertyValue::String("1 +".to_string()),
        )]),
    };
    fixture
        .project
        .get_node_mut(fixture.fmod_id)
        .context("Fmod Node must exist")?
        .set_property(FMOD_DIVISOR_INPUT_PORT.to_string(), malformed_expression)
        .map_err(|error| anyhow!(error))?;

    assert!(
        video_time(&evaluate(&fixture.project, 5)?).is_none(),
        "a failed authored Expression must become NoOutput at the frame boundary"
    );
    Ok(())
}

#[test]
fn clip_local_time_is_computed_before_the_explicit_node_remap() -> Result<()> {
    let fixture = time_graph_fixture(2.0, 0.25, 2.0, true)?;

    // Clip-local time = (3.6 - 2.0) * 2.0 + 0.25 = 3.45, followed by
    // the Media Node's explicit fmod remap = 0.45.
    assert_close(
        video_time(&evaluate(&fixture.project, 36)?)
            .context("local-time frame must contain Video time")?,
        0.45,
    );
    Ok(())
}

#[test]
fn fmod_factory_descriptor_ports_and_roundtrip_are_authoritative() -> Result<()> {
    let fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    let node = fixture
        .project
        .get_node(fixture.fmod_id)
        .context("Fmod Node must exist")?;
    assert_eq!(node.content(), &NodeContent::Value(ValueContent::Fmod));
    assert_eq!(
        node.properties()
            .get(FMOD_DIVISOR_INPUT_PORT)
            .and_then(Property::value),
        Some(&PropertyValue::Number(OrderedFloat(1.0)))
    );
    let divisor_definition = ValueContent::Fmod.property_definitions()[0].clone();
    assert_eq!(divisor_definition.name(), FMOD_DIVISOR_INPUT_PORT);
    assert!(
        divisor_definition
            .validate_value(&PropertyValue::Number(OrderedFloat(0.0)))
            .is_ok()
    );

    let ports = fixture
        .project
        .port_definitions(PortOwner::Node(fixture.fmod_id));
    for (key, direction) in [
        (FMOD_X_INPUT_PORT, PortDirection::Input),
        (FMOD_DIVISOR_INPUT_PORT, PortDirection::Input),
        (NUMBER_RESULT_OUTPUT_PORT, PortDirection::Output),
    ] {
        let port = ports
            .iter()
            .find(|port| port.key == key && port.direction == direction)
            .with_context(|| format!("{key} {direction:?} port must exist"))?;
        assert_eq!(port.data_type, PortDataType::Numeric);
        assert_eq!(port.exposure, PortExposure::Graph);
    }
    assert!(ports.iter().all(|port| port.key != TIME_PORT));

    let loaded = Project::load(&fixture.project.save()?)?;
    assert_eq!(loaded, fixture.project);
    assert_eq!(
        loaded.port_definitions(PortOwner::Node(fixture.fmod_id)),
        ports
    );
    Ok(())
}

#[test]
fn media_factory_requires_time_but_not_authored_frame_or_fps() -> Result<()> {
    let fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    let media = fixture
        .project
        .get_node(fixture.media_id)
        .context("Media Node must exist")?;
    let ports = fixture
        .project
        .port_definitions(PortOwner::Node(fixture.media_id));
    let time = ports
        .iter()
        .find(|port| port.key == TIME_PORT && port.direction == PortDirection::Input)
        .context("Media Node must expose its explicit Time input")?;
    assert_eq!(time.data_type, PortDataType::Number);
    assert_eq!(time.exposure, PortExposure::Graph);
    assert!(
        ports
            .iter()
            .all(|port| !matches!(port.key.as_str(), FRAME_PORT | FPS_PORT)),
        "Frame and FPS remain inherited read-only context, never Media authoring ports"
    );
    assert!(media.properties().get(FRAME_PORT).is_none());
    assert!(media.properties().get(FPS_PORT).is_none());
    Ok(())
}

#[test]
fn scalar_connections_keep_cycle_validation() -> Result<()> {
    let mut fixture = time_graph_fixture(0.0, 0.0, 1.0, true)?;
    assert!(matches!(
        fixture.project.connect_ports(
            PortAddress::new(PortOwner::Node(fixture.fmod_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(fixture.fmod_id), FMOD_X_INPUT_PORT),
        ),
        Err(ProjectGraphError::ConnectionCycle { .. })
    ));
    assert!(fixture.project.validate_connections().is_empty());
    Ok(())
}

#[test]
fn eval_output_distinguishes_no_output_from_numeric_zero() {
    assert_ne!(
        EvalOutput::NoOutput,
        EvalOutput::Produced(PropertyValue::Number(OrderedFloat(0.0)))
    );
}
