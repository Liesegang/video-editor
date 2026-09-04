use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use super::FrameEvaluator;
use super::path_graph::finalize_boolean_path_result;
use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};
use crate::model::project::connection::{
    DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY, LIST_ITEMS_INPUT_PORT, LIST_OUTPUT_PORT,
    PATH_OUTPUT_PORT, PATHS_INPUT_PORT,
};
use crate::model::project::{EvalOutput, NodeContainer, PortAddress, PortOwner, Project};
use crate::model::property::{Property, PropertyValue};
use crate::model::{Clip, Composition, DataContent, ListContent, Node, PathOperationContent};
use crate::plugin::PluginManager;

struct PathFixture {
    project: Project,
    clip_id: Uuid,
}

impl PathFixture {
    fn new() -> Self {
        let mut project = Project::new("Path operation graph");
        let (composition, track) = Composition::new("Main", 64, 64, 30.0, 4.0);
        let track_id = track.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let clip = Clip::new("Path scope", 0.0, 2.0);
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
}

fn rectangle(left: f64, top: f64, right: f64, bottom: f64) -> PathValue {
    PathValue::new(
        FillRule::NonZero,
        vec![PathContour::new(
            PathPoint::new(left, top),
            vec![
                PathSegment::line(PathPoint::new(right, top)),
                PathSegment::line(PathPoint::new(right, bottom)),
                PathSegment::line(PathPoint::new(left, bottom)),
            ],
            true,
        )],
    )
    .unwrap()
}

fn path_leaf(fixture: &mut PathFixture, name: &str, value: PathValue) -> Uuid {
    let mut node = Node::new_data(name, DataContent::Path);
    node.set_property(
        DATA_VALUE_PROPERTY.to_string(),
        Property::constant(PropertyValue::Path(value)),
    )
    .unwrap();
    fixture.add(node)
}

fn union_paths(fixture: &mut PathFixture, paths: Vec<PathValue>) -> Uuid {
    let list = fixture.add(Node::new_list("Paths", ListContent::Make));
    for (index, value) in paths.into_iter().enumerate() {
        let source = path_leaf(fixture, &format!("Path {index}"), value);
        fixture
            .project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), DATA_VALUE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(list), LIST_ITEMS_INPUT_PORT),
            )
            .unwrap();
    }
    let union = fixture.add(Node::new_path_operation(
        "Union Path",
        PathOperationContent::Union,
    ));
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(list), LIST_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(union), PATHS_INPUT_PORT),
        )
        .unwrap();
    union
}

#[test]
fn union_path_evaluates_canonical_list_data_and_survives_project_roundtrip() {
    let mut fixture = PathFixture::new();
    let left = path_leaf(&mut fixture, "Left", rectangle(0.0, 0.0, 10.0, 10.0));
    let right = path_leaf(&mut fixture, "Right", rectangle(5.0, 0.0, 15.0, 10.0));
    let list = fixture.add(Node::new_list("Paths", ListContent::Make));
    let union = fixture.add(Node::new_path_operation(
        "Union Path",
        PathOperationContent::Union,
    ));
    for source in [left, right] {
        fixture
            .project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), DATA_VALUE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(list), LIST_ITEMS_INPUT_PORT),
            )
            .unwrap();
    }
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(list), LIST_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(union), PATHS_INPUT_PORT),
        )
        .unwrap();
    assert!(fixture.project.validate_connections().is_empty());

    let output = fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5);
    assert!(matches!(
        &output,
        EvalOutput::Produced(PropertyValue::Path(_))
    ));
    let EvalOutput::Produced(PropertyValue::Path(output)) = output else {
        return;
    };
    assert_eq!(output.fill_rule(), FillRule::NonZero);
    assert!(output.contours().iter().all(PathContour::is_closed));
    let backend = crate::core::rendering::path_geometry::to_skia_path(&output).unwrap();
    assert_eq!(
        backend.compute_tight_bounds(),
        skia_safe::Rect::new(0.0, 0.0, 15.0, 10.0)
    );
    assert!(backend.contains((2.0, 5.0)));
    assert!(backend.contains((13.0, 5.0)));
    assert!(!backend.contains((20.0, 5.0)));

    fixture.project = Project::load(&fixture.project.save().unwrap()).unwrap();
    assert!(matches!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Path(_))
    ));
}

#[test]
fn union_path_empty_mixed_disabled_bypassed_and_out_of_range_are_fail_closed() {
    let mut fixture = PathFixture::new();
    let empty = fixture.add(Node::new_list("Empty Paths", ListContent::Make));
    let union = fixture.add(Node::new_path_operation(
        "Union Path",
        PathOperationContent::Union,
    ));
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(empty), LIST_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(union), PATHS_INPUT_PORT),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Path(PathValue::empty(FillRule::NonZero)))
    );
    assert_eq!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 3.0),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(union).unwrap().enabled = false;
    assert_eq!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(union).unwrap().enabled = true;
    fixture.project.get_node_mut(union).unwrap().bypassed = true;
    assert_eq!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(union).unwrap().bypassed = false;

    let color = fixture.add(Node::new_data("Not a Path", DataContent::Color));
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(color), DATA_VALUE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(empty), LIST_ITEMS_INPUT_PORT),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "Union Path must not silently discard non-Path list members"
    );
}

#[test]
fn union_path_accepts_closed_conics_and_normalizes_boolean_fill_semantics() {
    let conic = PathValue::new(
        FillRule::EvenOdd,
        vec![PathContour::new(
            PathPoint::new(0.0, 10.0),
            vec![
                PathSegment::conic(
                    PathPoint::new(10.0, -10.0),
                    PathPoint::new(20.0, 10.0),
                    0.375,
                ),
                PathSegment::line(PathPoint::new(0.0, 10.0)),
            ],
            true,
        )],
    )
    .unwrap();
    let mut fixture = PathFixture::new();
    let union = union_paths(&mut fixture, vec![conic]);

    let EvalOutput::Produced(PropertyValue::Path(output)) =
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5)
    else {
        panic!("a finite closed conic must produce canonical Path data");
    };
    assert_eq!(output.fill_rule(), FillRule::NonZero);
    assert!(!output.contours().is_empty());
    assert!(output.contours().iter().all(PathContour::is_closed));
    output.validate().unwrap();
    let backend = crate::core::rendering::path_geometry::to_skia_path(&output).unwrap();
    assert!(backend.is_finite());
    assert!(!backend.is_empty());
}

#[test]
fn union_path_rejects_invalid_or_out_of_range_coordinates_without_fallback() {
    assert!(
        PathValue::new(
            FillRule::NonZero,
            vec![PathContour::new(
                PathPoint::new(f64::INFINITY, 0.0),
                Vec::new(),
                false,
            )],
        )
        .is_err(),
        "non-finite canonical geometry must be rejected before graph evaluation"
    );

    let mut fixture = PathFixture::new();
    let union = union_paths(&mut fixture, vec![rectangle(0.0, 0.0, 1.0e300, 10.0)]);
    assert_eq!(
        fixture.evaluate(union, PATH_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "finite f64 coordinates outside the backend f32 domain must not reuse the input"
    );
    assert_eq!(
        finalize_boolean_path_result(None),
        EvalOutput::NoOutput,
        "a native PathOps failure must not silently return a plausible Path"
    );
}
