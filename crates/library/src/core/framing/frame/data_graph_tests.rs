use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use uuid::Uuid;

use super::FrameEvaluator;
use crate::model::frame::color::Color;
use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};
use crate::model::project::connection::{
    DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY, LIST_ITEMS_INPUT_PORT, LIST_OUTPUT_PORT,
};
use crate::model::project::{EvalOutput, NodeContainer, PortAddress, PortOwner, Project};
use crate::model::property::{ColorSpaceRef, ColorValue, Property, PropertyValue};
use crate::model::{Clip, Composition, DataContent, ListContent, Node};
use crate::plugin::PluginManager;

struct DataFixture {
    project: Project,
    clip_id: Uuid,
}

impl DataFixture {
    fn new() -> Self {
        let mut project = Project::new("Data graph");
        let (composition, track) = Composition::new("Main", 64, 64, 30.0, 4.0);
        let track_id = track.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let clip = Clip::new("Data scope", 0.0, 2.0);
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

fn complete_path() -> PathValue {
    PathValue::new(
        FillRule::EvenOdd,
        vec![
            PathContour::new(
                PathPoint::new(-2.5, 4.0),
                vec![
                    PathSegment::conic(PathPoint::new(0.25, 9.5), PathPoint::new(7.0, -3.0), 0.375),
                    PathSegment::cubic(
                        PathPoint::new(8.0, 1.0),
                        PathPoint::new(9.0, 2.0),
                        PathPoint::new(10.0, 3.0),
                    ),
                ],
                true,
            ),
            PathContour::new(
                PathPoint::new(20.0, 30.0),
                vec![PathSegment::quadratic(
                    PathPoint::new(22.0, 35.0),
                    PathPoint::new(25.0, 31.0),
                )],
                false,
            ),
        ],
    )
    .unwrap()
}

#[test]
fn color_leaf_preserves_hdr_tag_and_straight_alpha_through_project_roundtrip() {
    let mut fixture = DataFixture::new();
    let value = ColorValue::new(
        ColorSpaceRef::new("scene_linear_ap1").unwrap(),
        [-0.5, 4.25, 0.125, 0.375],
    )
    .unwrap();
    let mut node = Node::new_data("HDR Color", DataContent::Color);
    node.set_property(
        DATA_VALUE_PROPERTY.to_string(),
        Property::constant(PropertyValue::ColorValue(value.clone())),
    )
    .unwrap();
    let node_id = fixture.add(node);
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::ColorValue(value.clone()))
    );

    let encoded = serde_json::to_string(&fixture.project).unwrap();
    assert!(encoded.contains("scene_linear_ap1"));
    let restored: Project = serde_json::from_str(&encoded).unwrap();
    fixture.project = restored;
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::ColorValue(value))
    );
}

#[test]
fn path_leaf_preserves_fill_closure_and_arbitrary_conic_weight() {
    let mut fixture = DataFixture::new();
    let path = complete_path();
    let mut node = Node::new_data("Canonical Path", DataContent::Path);
    node.set_property(
        DATA_VALUE_PROPERTY.to_string(),
        Property::constant(PropertyValue::Path(path.clone())),
    )
    .unwrap();
    let node_id = fixture.add(node);
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Path(path.clone()))
    );

    let restored: Project =
        serde_json::from_str(&serde_json::to_string(&fixture.project).unwrap()).unwrap();
    fixture.project = restored;
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Path(path))
    );
}

#[test]
fn malformed_canonical_data_survives_project_roundtrip_and_produces_no_output() -> Result<()> {
    let path = serde_json::to_value(complete_path())?;
    let mut unknown_segment = path.clone();
    unknown_segment["contours"][0]["segments"][0]["kind"] = serde_json::json!("unknown_segment");
    let mut invalid_weight = path;
    invalid_weight["contours"][0]["segments"][0]["weight"] = serde_json::json!("not-a-number");
    let cases = [
        (
            DataContent::Color,
            serde_json::json!({
                "$type": "color_value",
                "space": "srgb",
                "rgba": [0.0, 0.0, 0.0, 1.5],
            }),
        ),
        (
            DataContent::Color,
            serde_json::json!({
                "$type": "color_value",
                "space": "",
                "rgba": [0.0, 0.0, 0.0, 1.0],
            }),
        ),
        (
            DataContent::Color,
            serde_json::json!({
                "$type": "color_value",
                "space": null,
                "rgba": [0.0, 0.0, 0.0, 1.0],
            }),
        ),
        (
            DataContent::Color,
            serde_json::json!({
                "$type": "color_value",
                "space": "srgb",
                "rgba": [0.0, 0.0, 0.0, u64::MAX],
            }),
        ),
        (DataContent::Path, unknown_segment),
        (DataContent::Path, invalid_weight),
    ];

    for (content, malformed) in cases {
        let mut fixture = DataFixture::new();
        let node_id = fixture.add(Node::new_data("Malformed Data", content));
        let mut encoded = serde_json::to_value(&fixture.project)?;
        encoded["nodes"][node_id.to_string()]["properties"][DATA_VALUE_PROPERTY]["properties"]["value"] =
            malformed.clone();

        let loaded = Project::load(&serde_json::to_string(&encoded)?)?;
        let authored = loaded
            .get_node(node_id)
            .context("malformed Data Node must survive Project load")?
            .properties()
            .get(DATA_VALUE_PROPERTY)
            .and_then(Property::value)
            .context("malformed Data value must survive Project load")?;
        assert!(matches!(authored, PropertyValue::Map(_)));
        assert_eq!(serde_json::Value::from(authored), malformed);

        let reloaded = Project::load(&loaded.save()?)?;
        let restored = reloaded
            .get_node(node_id)
            .context("malformed Data Node must survive Project reload")?
            .properties()
            .get(DATA_VALUE_PROPERTY)
            .and_then(Property::value)
            .context("malformed Data value must survive Project reload")?;
        assert_eq!(serde_json::Value::from(restored), malformed);

        fixture.project = reloaded;
        assert_eq!(
            fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
            EvalOutput::NoOutput
        );
    }
    Ok(())
}

#[test]
fn typed_data_outputs_flow_through_the_shared_heterogeneous_list_runtime() {
    let mut fixture = DataFixture::new();
    let color = ColorValue::new(
        ColorSpaceRef::new("acescg").unwrap(),
        [-0.125, 8.0, 0.25, 0.75],
    )
    .unwrap();
    let path = complete_path();
    let mut color_node = Node::new_data("Color", DataContent::Color);
    color_node
        .set_property(
            DATA_VALUE_PROPERTY.to_string(),
            Property::constant(PropertyValue::ColorValue(color.clone())),
        )
        .unwrap();
    let mut path_node = Node::new_data("Path", DataContent::Path);
    path_node
        .set_property(
            DATA_VALUE_PROPERTY.to_string(),
            Property::constant(PropertyValue::Path(path.clone())),
        )
        .unwrap();
    let color_id = fixture.add(color_node);
    let path_id = fixture.add(path_node);
    let list_id = fixture.add(Node::new_list("List", ListContent::Make));
    for source in [color_id, path_id] {
        fixture
            .project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), DATA_VALUE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(list_id), LIST_ITEMS_INPUT_PORT),
            )
            .unwrap();
    }
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(list_id, LIST_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Array(vec![
            PropertyValue::ColorValue(color),
            PropertyValue::Path(path),
        ]))
    );
}

#[test]
fn malformed_disabled_bypassed_and_out_of_range_are_no_output() {
    let mut fixture = DataFixture::new();
    let mut node = Node::new_data("Color", DataContent::Color);
    assert!(
        node.set_property(
            DATA_VALUE_PROPERTY.to_string(),
            Property::constant(PropertyValue::Color(Color {
                r: 1,
                g: 2,
                b: 3,
                a: 4,
            })),
        )
        .is_err(),
        "public authoring must reject the lossy legacy Color substitution"
    );
    let node_id = fixture.add(node);
    let mut malformed = serde_json::to_value(&fixture.project).unwrap();
    malformed["nodes"][node_id.to_string()]["properties"][DATA_VALUE_PROPERTY]["properties"]["value"] =
        serde_json::to_value(PropertyValue::Color(Color {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        }))
        .unwrap();
    fixture.project = serde_json::from_value(malformed).unwrap();
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "legacy 8-bit Color must not be reinterpreted as canonical ColorValue"
    );

    let canonical = ColorValue::new(ColorSpaceRef::srgb(), [0.1, 0.2, 0.3, 0.4]).unwrap();
    fixture
        .project
        .get_node_mut(node_id)
        .unwrap()
        .set_property(
            DATA_VALUE_PROPERTY.to_string(),
            Property::constant(PropertyValue::ColorValue(canonical)),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 3.0),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(node_id).unwrap().enabled = false;
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(node_id).unwrap().enabled = true;
    fixture.project.get_node_mut(node_id).unwrap().bypassed = true;
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
}

#[test]
fn persisted_missing_value_property_is_loadable_but_evaluates_to_no_output() {
    let mut fixture = DataFixture::new();
    let node_id = fixture.add(Node::new_data("Path", DataContent::Path));
    let mut encoded = serde_json::to_value(&fixture.project).unwrap();
    encoded["nodes"][node_id.to_string()]["properties"] = serde_json::json!({});
    fixture.project = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        fixture.evaluate(node_id, DATA_VALUE_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
}
