use std::collections::HashSet;
use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::FrameEvaluator;
use crate::error::LibraryError;
use crate::model::project::{
    Composition, DURATION_PORT, EvalOutput, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, FPS_PORT,
    FRAME_PORT, NUMBER_RESULT_OUTPUT_PORT, NodeContainer, PortAddress, PortOwner, Project,
    ProjectConnection, RESOLUTION_PORT, TIME_PORT,
};
use crate::model::property::{Property, PropertyValue, Vec2};
use crate::model::{Clip, CompositionInstanceContent, Node, NodeContent};
use crate::plugin::PluginManager;

fn evaluate_numeric_output_from(
    mut node: Node,
    left: f64,
    right: f64,
    source_port: Option<&str>,
) -> EvalOutput<PropertyValue> {
    let mut project = Project::new("fmod semantics");
    let (composition, track) = Composition::new("main", 32, 32, 30.0, 2.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let mut clip = Clip::new("clip", 0.0, 1.0);
    clip.trim_in = OrderedFloat(left - 0.5);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let NodeContent::Value(value) = node.content() else {
        return EvalOutput::NoOutput;
    };
    let value = *value;
    node.set_property(
        value.secondary_input().to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(right))),
    )
    .unwrap();
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    if let Some(source_port) = source_port {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), source_port),
                PortAddress::new(PortOwner::Node(node_id), value.primary_input()),
            )
            .unwrap();
    }

    let plugin_manager = Arc::new(PluginManager::default());
    let evaluator = FrameEvaluator::new(
        &project,
        &project.compositions[0],
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    );
    evaluator
        .resolve_metadata_value(
            &PortAddress::new(PortOwner::Node(node_id), NUMBER_RESULT_OUTPUT_PORT),
            0.5,
            &mut HashSet::new(),
        )
        .unwrap()
}

fn evaluate_numeric_output(node: Node, left: f64, right: f64) -> EvalOutput<PropertyValue> {
    evaluate_numeric_output_from(node, left, right, Some(TIME_PORT))
}

fn evaluate_fmod_output(x: f64, divisor: f64) -> EvalOutput<PropertyValue> {
    evaluate_numeric_output(Node::new_fmod("Fmod"), x, divisor)
}

#[test]
fn fmod_uses_rust_remainder_sign_semantics_for_all_sign_pairs() {
    for (x, divisor, expected) in [
        (5.5, 2.0, 1.5),
        (5.5, -2.0, 1.5),
        (-5.5, 2.0, -1.5),
        (-5.5, -2.0, -1.5),
    ] {
        assert_eq!(
            evaluate_fmod_output(x, divisor),
            EvalOutput::Produced(PropertyValue::Number(OrderedFloat(expected))),
            "{x} % {divisor} must match Rust/C fmod semantics"
        );
    }
}

#[test]
fn fmod_non_finite_inputs_and_zero_divisors_produce_no_output() {
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(evaluate_fmod_output(x, 2.0), EvalOutput::NoOutput);
    }
    for divisor in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(evaluate_fmod_output(5.5, divisor), EvalOutput::NoOutput);
    }
}

#[test]
fn basic_numeric_nodes_execute_through_the_shared_graph_evaluator() {
    for (node, expected) in [
        (Node::new_add("Add"), 8.0),
        (Node::new_subtract("Subtract"), 4.0),
        (Node::new_multiply("Multiply"), 12.0),
        (Node::new_divide("Divide"), 3.0),
    ] {
        assert_eq!(
            evaluate_numeric_output(node, 6.0, 2.0),
            EvalOutput::Produced(PropertyValue::Number(OrderedFloat(expected)))
        );
    }
    assert_eq!(
        evaluate_numeric_output(Node::new_divide("Divide"), 6.0, -0.0),
        EvalOutput::NoOutput
    );
}

#[test]
fn numeric_bypass_routes_primary_input_and_preserves_no_output_gates() {
    let mut bypassed = Node::new_divide("bypassed Divide");
    bypassed.bypassed = true;
    assert_eq!(
        evaluate_numeric_output_from(bypassed.clone(), 6.0, 0.0, Some(TIME_PORT)),
        EvalOutput::Produced(PropertyValue::Number(OrderedFloat(6.0)))
    );
    assert_eq!(
        evaluate_numeric_output_from(bypassed.clone(), 0.0, 0.0, Some(RESOLUTION_PORT)),
        EvalOutput::Produced(PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(32.0),
            y: OrderedFloat(32.0),
        }))
    );
    assert_eq!(
        evaluate_numeric_output_from(bypassed.clone(), 6.0, 0.0, None),
        EvalOutput::NoOutput
    );

    bypassed.enabled = false;
    assert_eq!(
        evaluate_numeric_output_from(bypassed, 6.0, 0.0, Some(TIME_PORT)),
        EvalOutput::NoOutput
    );
}

#[test]
fn composition_instance_metadata_uses_the_explicitly_timed_target_scope() {
    let mut project = Project::new("composition instance metadata");
    let (target, target_track) = Composition::new("target", 640, 360, 24.0, 4.0);
    let target_id = target.id;
    assert!(
        project.add_track(target_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(target).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let (parent, parent_track) = Composition::new("parent", 320, 180, 30.0, 10.0);
    let parent_id = parent.id;
    let parent_track_id = parent_track.id;
    assert!(
        project.add_track(parent_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(parent).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let mut clip = Clip::new("instance", 2.0, 2.0);
    clip.trim_in = OrderedFloat(1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project
        .attach_clip_to_track(parent_track_id, clip_id)
        .unwrap();
    let instance = Node::new_composition_instance(
        "instance",
        CompositionInstanceContent {
            composition_id: target_id,
        },
    );
    let instance_id = instance.id;
    project.add_node(instance);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), instance_id)
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(instance_id))
        .unwrap();

    let mut fmod = Node::new_fmod("one-second loop");
    fmod.set_property(
        FMOD_DIVISOR_INPUT_PORT.to_string(),
        Property::expression(
            "value".to_string(),
            PropertyValue::Number(OrderedFloat(1.0)),
        ),
    )
    .unwrap();
    let fmod_id = fmod.id;
    project.add_node(fmod);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), fmod_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(instance_id), TIME_PORT),
        )
        .unwrap();
    assert!(project.validate_connections().is_empty());

    let plugin_manager = Arc::new(PluginManager::default());
    let evaluator = FrameEvaluator::new(
        &project,
        project.get_composition(parent_id).unwrap(),
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    );
    for (port, expected) in [
        (TIME_PORT, PropertyValue::Number(OrderedFloat(0.5))),
        (FRAME_PORT, PropertyValue::Integer(12)),
        (FPS_PORT, PropertyValue::Number(OrderedFloat(24.0))),
        (DURATION_PORT, PropertyValue::Number(OrderedFloat(4.0))),
        (
            RESOLUTION_PORT,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(640.0),
                y: OrderedFloat(360.0),
            }),
        ),
    ] {
        assert_eq!(
            evaluator
                .resolve_metadata_value(
                    &PortAddress::new(PortOwner::Node(instance_id), port),
                    2.5,
                    &mut HashSet::new(),
                )
                .unwrap(),
            EvalOutput::Produced(expected),
            "Composition Instance {port} must describe its evaluated target scope"
        );
    }
}

#[test]
fn value_resolver_detects_a_cycle_even_when_called_without_project_validation() {
    let mut project = Project::new("direct value resolver cycle");
    let (composition, track) = Composition::new("main", 32, 32, 30.0, 1.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let clip = Clip::new("clip", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let first = Node::new_fmod("first");
    let first_id = first.id;
    let second = Node::new_fmod("second");
    let second_id = second.id;
    for node in [first, second] {
        let id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), id)
            .unwrap();
    }

    // Push malformed cyclic state directly to exercise the runtime guard;
    // normal Project::connect_ports rejects either self/cyclic edge first.
    project.connections.extend([
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(second_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(first_id), FMOD_X_INPUT_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(first_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(second_id), FMOD_X_INPUT_PORT),
            0,
        ),
    ]);
    assert!(!project.validate_connections().is_empty());

    let plugin_manager = Arc::new(PluginManager::default());
    let evaluator = FrameEvaluator::new(
        &project,
        &project.compositions[0],
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    );
    let error = evaluator
        .resolve_metadata_value(
            &PortAddress::new(PortOwner::Node(first_id), NUMBER_RESULT_OUTPUT_PORT),
            0.0,
            &mut HashSet::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        LibraryError::Validation(message)
            if message.to_ascii_lowercase().contains("cycle")
    ));
}
