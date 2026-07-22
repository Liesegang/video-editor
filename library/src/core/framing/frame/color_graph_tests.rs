use std::collections::HashSet;
use std::sync::Arc;

use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::FrameEvaluator;
use crate::model::project::{
    DURATION_PORT, EvalOutput, NodeContainer, PortAddress, PortOwner, Project, TIME_PORT,
};
use crate::model::property::{ColorSpaceRef, ColorValue, Keyframe, Property, PropertyValue};
use crate::model::{
    COLOR_ALPHA_PORT, COLOR_BLUE_PORT, COLOR_GREEN_PORT, COLOR_MIX_FACTOR_PORT,
    COLOR_MIX_LEFT_PORT, COLOR_MIX_RIGHT_PORT, COLOR_RED_PORT, COLOR_SPACE_PORT, COLOR_VALUE_PORT,
    Clip, ColorContent, Composition, Node,
};
use crate::plugin::PluginManager;

struct ColorFixture {
    project: Project,
    clip_id: Uuid,
}

impl ColorFixture {
    fn new() -> Self {
        let mut project = Project::new("Color graph");
        let (composition, track) = Composition::new("Main", 64, 64, 30.0, 4.0);
        let track_id = track.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let clip = Clip::new("Color scope", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        Self { project, clip_id }
    }

    fn add(&mut self, node: Node) -> Uuid {
        let id = node.id;
        self.project.add_node(node);
        self.project
            .attach_node_to_container(NodeContainer::Clip(self.clip_id), id)
            .unwrap();
        id
    }

    fn evaluate(&self, node_id: Uuid, output: &str, time: f64) -> EvalOutput<PropertyValue> {
        let plugins = Arc::new(PluginManager::default());
        FrameEvaluator::new(
            &self.project,
            &self.project.compositions[0],
            plugins.get_property_evaluators(),
            plugins.as_ref(),
        )
        .resolve_metadata_value(
            &PortAddress::new(PortOwner::Node(node_id), output),
            time,
            &mut HashSet::new(),
        )
        .unwrap()
    }

    fn connect(&mut self, from: Uuid, output: &str, to: Uuid, input: &str) {
        self.project
            .connect_ports(
                PortAddress::new(PortOwner::Node(from), output),
                PortAddress::new(PortOwner::Node(to), input),
            )
            .unwrap();
    }
}

fn set(node: &mut Node, key: &str, value: PropertyValue) {
    node.set_property(key.to_string(), Property::constant(value))
        .unwrap();
}

fn compose_node(space: &str, rgba: [f64; 4]) -> Node {
    let mut node = Node::new_color("Compose", ColorContent::Compose);
    set(
        &mut node,
        COLOR_SPACE_PORT,
        PropertyValue::String(space.to_string()),
    );
    for (key, value) in [
        (COLOR_RED_PORT, rgba[0]),
        (COLOR_GREEN_PORT, rgba[1]),
        (COLOR_BLUE_PORT, rgba[2]),
        (COLOR_ALPHA_PORT, rgba[3]),
    ] {
        set(&mut node, key, PropertyValue::Number(OrderedFloat(value)));
    }
    node
}

#[test]
fn compose_and_split_roundtrip_hdr_straight_rgba_and_space_losslessly() {
    let mut fixture = ColorFixture::new();
    let expected = ColorValue::new(
        ColorSpaceRef::new("scene_linear_ap1").unwrap(),
        [-2.5, 7.25, 0.125, 0.375],
    )
    .unwrap();
    let compose_id = fixture.add(compose_node("scene_linear_ap1", expected.rgba()));
    let split_id = fixture.add(Node::new_color("Split", ColorContent::Split));
    fixture.connect(compose_id, COLOR_VALUE_PORT, split_id, COLOR_VALUE_PORT);
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(compose_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::ColorValue(expected.clone()))
    );
    for (port, value) in [
        (COLOR_RED_PORT, -2.5),
        (COLOR_GREEN_PORT, 7.25),
        (COLOR_BLUE_PORT, 0.125),
        (COLOR_ALPHA_PORT, 0.375),
    ] {
        assert_eq!(
            fixture.evaluate(split_id, port, 0.5),
            EvalOutput::Produced(PropertyValue::Number(OrderedFloat(value)))
        );
    }
    assert_eq!(
        fixture.evaluate(split_id, COLOR_SPACE_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::String("scene_linear_ap1".to_string()))
    );

    fixture.project = Project::load(&fixture.project.save().unwrap()).unwrap();
    assert_eq!(
        fixture.evaluate(compose_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::ColorValue(expected))
    );
}

#[test]
fn mix_uses_wired_colors_and_factor_without_premultiplying_alpha() {
    let mut fixture = ColorFixture::new();
    let left_id = fixture.add(compose_node("acescg", [-4.0, 2.0, 0.0, 0.2]));
    let right_id = fixture.add(compose_node("acescg", [8.0, -2.0, 4.0, 0.8]));
    let mix_id = fixture.add(Node::new_color("Mix", ColorContent::Mix));
    fixture.connect(left_id, COLOR_VALUE_PORT, mix_id, COLOR_MIX_LEFT_PORT);
    fixture.connect(right_id, COLOR_VALUE_PORT, mix_id, COLOR_MIX_RIGHT_PORT);
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(fixture.clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(mix_id), COLOR_MIX_FACTOR_PORT),
        )
        .unwrap();
    assert!(fixture.project.validate_connections().is_empty());
    let expected = ColorValue::new(
        ColorSpaceRef::new("acescg").unwrap(),
        [-1.0, 1.0, 1.0, 0.2 + (0.8 - 0.2) * 0.25],
    )
    .unwrap();
    assert_eq!(
        fixture.evaluate(mix_id, COLOR_VALUE_PORT, 0.25),
        EvalOutput::Produced(PropertyValue::ColorValue(expected)),
        "RGB and alpha must interpolate independently in straight-alpha form"
    );
}

#[test]
fn explicit_time_input_drives_mix_factor_keyframes() {
    let mut fixture = ColorFixture::new();
    let mut mix = Node::new_color("Timed Mix", ColorContent::Mix);
    mix.set_property(
        COLOR_MIX_FACTOR_PORT.to_string(),
        Property::keyframe(vec![
            Keyframe::new(
                0.0,
                PropertyValue::Number(OrderedFloat(0.0)),
                Default::default(),
            ),
            Keyframe::new(
                2.0,
                PropertyValue::Number(OrderedFloat(1.0)),
                Default::default(),
            ),
        ]),
    )
    .unwrap();
    let mix_id = fixture.add(mix);
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(fixture.clip_id), DURATION_PORT),
            PortAddress::new(PortOwner::Node(mix_id), TIME_PORT),
        )
        .unwrap();
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(mix_id, COLOR_VALUE_PORT, 0.25),
        EvalOutput::Produced(PropertyValue::ColorValue(
            ColorValue::new(ColorSpaceRef::srgb(), [1.0, 1.0, 1.0, 1.0]).unwrap()
        )),
        "the explicit Time wire must sample factor at Clip Duration (2.0), not global 0.25"
    );
}

#[test]
fn invalid_space_alpha_mixed_spaces_factor_disabled_and_bypass_are_no_output() {
    let mut fixture = ColorFixture::new();
    let mut invalid_space = compose_node("srgb", [0.0, 0.0, 0.0, 1.0]);
    set(
        &mut invalid_space,
        COLOR_SPACE_PORT,
        PropertyValue::String("   ".to_string()),
    );
    let invalid_space_id = fixture.add(invalid_space);
    assert_eq!(
        fixture.evaluate(invalid_space_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::NoOutput
    );

    let alpha_id = fixture.add(compose_node("srgb", [0.0, 0.0, 0.0, 1.0]));
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(fixture.clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(alpha_id), COLOR_ALPHA_PORT),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(alpha_id, COLOR_VALUE_PORT, 1.5),
        EvalOutput::NoOutput,
        "connected alpha outside [0, 1] must not be clamped"
    );

    let invalid_factor_id = fixture.add(Node::new_color("Invalid factor", ColorContent::Mix));
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(fixture.clip_id), DURATION_PORT),
            PortAddress::new(PortOwner::Node(invalid_factor_id), COLOR_MIX_FACTOR_PORT),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(invalid_factor_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::NoOutput,
        "connected mix factors outside [0, 1] must not be clamped"
    );

    let left_id = fixture.add(compose_node("srgb", [0.0, 0.0, 0.0, 1.0]));
    let right_id = fixture.add(compose_node("acescg", [1.0, 1.0, 1.0, 1.0]));
    let mut mix = Node::new_color("Mix", ColorContent::Mix);
    assert!(
        mix.set_property(
            COLOR_MIX_FACTOR_PORT.to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(f64::NAN))),
        )
        .is_err(),
        "non-finite mix factors must be rejected at the authored model boundary"
    );
    set(
        &mut mix,
        COLOR_MIX_FACTOR_PORT,
        PropertyValue::Number(OrderedFloat(0.5)),
    );
    let mix_id = fixture.add(mix);
    fixture.connect(left_id, COLOR_VALUE_PORT, mix_id, COLOR_MIX_LEFT_PORT);
    fixture.connect(right_id, COLOR_VALUE_PORT, mix_id, COLOR_MIX_RIGHT_PORT);
    assert_eq!(
        fixture.evaluate(mix_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::NoOutput,
        "Color-space conversion must never be guessed"
    );

    fixture.project.get_node_mut(mix_id).unwrap().enabled = false;
    assert_eq!(
        fixture.evaluate(mix_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(mix_id).unwrap().enabled = true;
    fixture.project.get_node_mut(mix_id).unwrap().bypassed = true;
    assert!(!fixture.project.get_node(mix_id).unwrap().supports_bypass());
    assert_eq!(
        fixture.evaluate(mix_id, COLOR_VALUE_PORT, 0.5),
        EvalOutput::NoOutput
    );
}
