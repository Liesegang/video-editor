//! Conditions preserve the Point domain and ordinary uniform evaluation owner.

use super::particle_tests::{ParticleFixture, connection, point_scenes};
use super::point_tests::{PointNodes, point_fixture, replace_fixture_source_with_grid};
use super::{
    CompiledPointInstruction, RenderPlanCache, RenderPlanCompiler, evaluate_render_plan_frame,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{AutomationKeyframe, AutomationTrack, MediaTime, SourceRef};
use crate::model::conditional::ComparisonOperation;
use crate::model::node::{ConditionalNodeRole, Node, PointNodeRole};
use crate::model::point::{PointAttributeElementType, PointInstruction, PointRenderProgram};
use crate::model::project::PortDataType;
use crate::model::property::{Property, PropertyValue};
use crate::plugin::PluginManager;

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(value.into())
}

struct ConditionalNodes {
    points: PointNodes,
    compare: uuid::Uuid,
    mask: uuid::Uuid,
    select: uuid::Uuid,
}

fn fixture(count: usize, grid: bool) -> (ParticleFixture, ConditionalNodes) {
    let (mut fixture, points) = point_fixture(count);
    if grid {
        replace_fixture_source_with_grid(&mut fixture, &points, "random");
    }
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let mut compare = Node::new_catalog_node(
        ConditionalNodeRole::Compare(ComparisonOperation::Greater).catalog_id(),
    )
    .unwrap();
    compare
        .set_property("b".into(), Property::constant(number(0.5)))
        .unwrap();
    let mut mask = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Boolean).catalog_id(),
    )
    .unwrap();
    mask.name = "hot".into();
    let mut select =
        Node::new_catalog_node(ConditionalNodeRole::Select(PortDataType::Number).catalog_id())
            .unwrap();
    select
        .set_property("if_true".into(), Property::constant(number(1.0)))
        .unwrap();
    select
        .set_property("if_false".into(), Property::constant(number(0.0)))
        .unwrap();
    let (compare_id, mask_id, select_id) = (compare.id, mask.id, select.id);
    for node in [compare, mask, select] {
        definition.graph.nodes.insert(node.id, node);
    }
    definition.graph.connections.retain(|edge| {
        !(edge.from.node_id == points.store
            && (edge.to.node_id == points.renderer || edge.to.node_id == points.ramp))
    });
    definition.graph.connections.extend([
        connection(points.store, "points", mask_id, "points", 0),
        connection(points.store, "attribute", compare_id, "a", 0),
        connection(compare_id, "result", mask_id, "value", 0),
        connection(mask_id, "points", points.renderer, "particles", 0),
        connection(mask_id, "attribute", select_id, "condition", 0),
        connection(select_id, "result", points.ramp, "factor", 0),
    ]);
    // The existing Published parameter now controls the comparison threshold;
    // placement, shared Definition identity, and automation ownership do not change.
    let parameter = definition
        .interface
        .parameters
        .iter_mut()
        .find(|p| p.id == points.factor_parameter)
        .unwrap();
    parameter.target.node_id = compare_id;
    parameter.name = "Heat threshold".into();
    parameter.default_value = number(0.5);
    definition.topology_revision += 1;
    definition.interface_version += 1;
    fixture.project.validate().unwrap();
    (
        fixture,
        ConditionalNodes {
            points,
            compare: compare_id,
            mask: mask_id,
            select: select_id,
        },
    )
}

fn programs(fixture: &ParticleFixture, frame: u64) -> Vec<PointRenderProgram> {
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        frame,
        1.0,
        None,
    )
    .unwrap();
    point_scenes(&frame.items)
        .iter()
        .map(|scene| scene.point_program.clone().unwrap())
        .collect()
}

#[test]
fn boolean_attributes_and_selection_compile_in_grid_and_particle_domains() {
    for grid in [false, true] {
        let (fixture, _) = fixture(1, grid);
        let sampled = programs(&fixture, 15);
        let program = &sampled[0];
        assert_eq!(
            program.schema.attributes()[1].element_type(),
            PointAttributeElementType::Boolean
        );
        let types = program.register_types().unwrap();
        for (index, instruction) in program.instructions.iter().enumerate() {
            match instruction {
                PointInstruction::Compare { operation, .. } => {
                    assert_eq!(*operation, ComparisonOperation::Greater);
                    assert_eq!(types[index], PointAttributeElementType::Boolean);
                }
                PointInstruction::Select { .. } => {
                    assert_eq!(types[index], PointAttributeElementType::Number)
                }
                _ => {}
            }
        }
        assert!(
            program
                .instructions
                .iter()
                .any(|op| matches!(op, PointInstruction::Compare { .. }))
        );
        assert!(
            program
                .instructions
                .iter()
                .any(|op| matches!(op, PointInstruction::Select { .. }))
        );
        let json = serde_json::to_string(&fixture.project).unwrap();
        assert_eq!(fixture.project, serde_json::from_str(&json).unwrap());
    }
}

#[test]
fn condition_threshold_keyframes_and_sibling_overrides_reuse_the_definition() {
    let (mut fixture, nodes) = fixture(2, true);
    let mut cache = RenderPlanCache::default();
    let (first, _) = cache.compile(&fixture.project).unwrap();
    let register = first.module_definitions[&fixture.definition_id].point_renderers[&nodes.points.renderer]
        .point_program.as_ref().unwrap().instructions.iter().position(|op| matches!(op,
            CompiledPointInstruction::Uniform { node_id, port, .. } if *node_id == nodes.compare && port == "b")).unwrap();
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("Node Clip")
    };
    invocation.automation_tracks.insert(
        nodes.points.factor_parameter,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(MediaTime::zero(), number(0.0), EasingFunction::Linear),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    number(1.0),
                    EasingFunction::Linear,
                ),
            ],
        },
    );
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[1])
        .unwrap()
        .parameter_overrides
        .insert(nodes.points.factor_parameter, number(0.9));
    let (after, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert_eq!(stats.reused_definitions, 1);
    assert!(std::sync::Arc::ptr_eq(
        &first.module_definitions[&fixture.definition_id],
        &after.module_definitions[&fixture.definition_id]
    ));
    let sampled = programs(&fixture, 15);
    assert_eq!(
        sampled[0].instructions[register],
        PointInstruction::Constant { value: number(0.5) }
    );
    assert_eq!(
        sampled[1].instructions[register],
        PointInstruction::Constant { value: number(0.9) }
    );
}

#[test]
fn uniform_comparison_and_selection_use_the_normal_module_value_runtime() {
    let (mut fixture, nodes) = fixture(1, true);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|edge| {
        !(edge.to.node_id == nodes.compare && edge.to.port == "a"
            || edge.to.node_id == nodes.select && edge.to.port == "condition")
    });
    definition.graph.connections.push(connection(
        nodes.compare,
        "result",
        nodes.select,
        "condition",
        0,
    ));
    definition
        .graph
        .nodes
        .get_mut(&nodes.compare)
        .unwrap()
        .set_property("a".into(), Property::constant(number(0.75)))
        .unwrap();
    for (threshold, expected) in [(0.5, true), (0.8, false)] {
        fixture
            .project
            .module_instances
            .get_mut(&fixture.instance_ids[0])
            .unwrap()
            .parameter_overrides
            .insert(nodes.points.factor_parameter, number(threshold));
        let sampled = programs(&fixture, 15);
        let program = &sampled[0];
        assert!(!program.instructions.iter().any(|op| matches!(
            op,
            PointInstruction::Compare { .. } | PointInstruction::Select { .. }
        )));
        assert!(program.instructions.contains(&PointInstruction::Constant {
            value: PropertyValue::Boolean(expected)
        }));
        let color = crate::color_management::sample_gradient_at(
            &crate::model::property::GradientValue::default(),
            if expected { 1.0 } else { 0.0 },
        )
        .unwrap();
        assert_eq!(
            program.instructions[usize::from(program.color_register)],
            PointInstruction::Constant {
                value: PropertyValue::ColorValue(color)
            }
        );
    }
}

#[test]
fn varying_boolean_cannot_drive_numeric_properties_or_other_point_domains() {
    let (mut fixture, nodes) = fixture(1, true);
    let mut wrong_numeric = fixture.project.clone();
    let definition = wrong_numeric
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.push(connection(
        nodes.mask,
        "attribute",
        nodes.points.math,
        "b",
        0,
    ));
    let error = RenderPlanCompiler::compile(&wrong_numeric).unwrap_err();
    assert!(
        error.contains("Boolean") && error.contains("Numeric"),
        "{error}"
    );

    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let foreign_info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let (grid_id, info_id) = (grid.id, foreign_info.id);
    for node in [grid, foreign_info] {
        definition.graph.nodes.insert(node.id, node);
    }
    definition
        .graph
        .connections
        .retain(|edge| !(edge.to.node_id == nodes.compare && edge.to.port == "a"));
    definition.graph.connections.extend([
        connection(grid_id, "points", info_id, "points", 0),
        connection(info_id, "random", nodes.compare, "a", 0),
    ]);
    let error = RenderPlanCompiler::compile(&fixture.project).unwrap_err();
    assert!(error.contains("different Point domain"), "{error}");
}

#[test]
fn unselected_branch_still_requires_the_same_domain_and_a_matching_type() {
    let (mut fixture, nodes) = fixture(1, true);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let integer_store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Integer).catalog_id(),
    )
    .unwrap();
    let integer_id = integer_store.id;
    definition.graph.nodes.insert(integer_id, integer_store);
    definition.graph.connections.retain(|edge| {
        !(edge.from.node_id == nodes.mask && edge.to.node_id == nodes.points.renderer)
    });
    definition.graph.connections.extend([
        connection(nodes.mask, "points", integer_id, "points", 0),
        connection(integer_id, "points", nodes.points.renderer, "particles", 0),
        connection(integer_id, "attribute", nodes.select, "if_false", 0),
    ]);
    let error = RenderPlanCompiler::compile(&fixture.project).unwrap_err();
    assert!(error.contains("implicit varying conversions"), "{error}");
}

#[test]
fn select_number_rejects_two_late_bound_vector_branches_even_before_numeric_consumer() {
    let (mut fixture, nodes) = fixture(1, true);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let math = Node::new_multiply("Position scale");
    let length = Node::new_catalog_node(crate::model::node::NUMERIC_LENGTH_CATALOG_ID).unwrap();
    let (math_id, length_id) = (math.id, length.id);
    for node in [math, length] {
        definition.graph.nodes.insert(node.id, node);
    }
    definition.graph.connections.retain(|edge| {
        !(edge.from.node_id == nodes.select && edge.to.node_id == nodes.points.ramp)
    });
    definition.graph.connections.extend([
        connection(nodes.points.info, "position", math_id, "a", 0),
        connection(math_id, "result", nodes.select, "if_true", 0),
        connection(math_id, "result", nodes.select, "if_false", 0),
        connection(nodes.select, "result", length_id, "value", 0),
        connection(length_id, "result", nodes.points.ramp, "factor", 0),
    ]);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let error = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        15,
        1.0,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("requires Number") && error.contains("Vec3"),
        "{error}"
    );
}
