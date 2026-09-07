//! Cross-owner contracts for Point connections as ordinary Image graph endpoints.

use super::compiler::compile_module;
use super::particle_tests::{
    connection, has_group, particle_fixture, particle_renderer_and_output, point_scenes,
};
use super::point_connection_tests::use_line_endpoint;
use super::point_tests::point_fixture;
use super::{RenderPlanCompiler, evaluate_render_plan_frame, validate_module_node_name};
use crate::model::frame::entity::FrameGroupKind;
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, Node, PARTICLE_SPRITE_RENDERER_CATALOG_ID, POINT_CONNECTIONS_PORT,
    POINT_LINE_RENDERER_CATALOG_ID, POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::point::PointAttributeElementType;
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType};
use crate::plugin::PluginManager;

#[test]
fn line_color_rejects_a_point_field_from_a_different_source_domain() {
    let (mut fixture, nodes) = point_fixture(1);
    use_line_endpoint(&mut fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();

    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let grid_id = grid.id;
    let info_id = info.id;
    definition
        .graph
        .nodes
        .extend([(grid_id, grid), (info_id, info)]);
    definition
        .graph
        .connections
        .retain(|edge| !(edge.to.node_id == nodes.ramp && edge.to.port == COLOR_RAMP_FACTOR_PORT));
    definition.graph.connections.extend([
        connection(grid_id, POINT_SOURCE_PORT, info_id, POINT_SOURCE_PORT, 0),
        connection(info_id, "random", nodes.ramp, COLOR_RAMP_FACTOR_PORT, 0),
    ]);
    definition.topology_revision += 1;

    let error = compile_module(definition).unwrap_err();
    assert!(error.contains("different Point domain"), "{error}");
}

#[test]
fn line_image_flows_through_effect_and_merge_before_output() {
    let mut fixture = particle_fixture(1);
    let (_, output_node_id) = particle_renderer_and_output(&fixture);
    let (_, renderer_id) = use_line_endpoint(&mut fixture);
    let plugins = PluginManager::default();
    let blur = plugins
        .create_effect_operation_node("blur")
        .expect("Blur operation");
    let blur_id = blur.id;
    let merge = Node::new_merge("Line Merge");
    let merge_id = merge.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition
        .graph
        .connections
        .retain(|edge| !(edge.from.node_id == renderer_id && edge.to.node_id == output_node_id));
    definition
        .graph
        .nodes
        .extend([(blur_id, blur), (merge_id, merge)]);
    definition.graph.connections.extend([
        connection(renderer_id, IMAGE_OUTPUT_PORT, blur_id, IMAGE_INPUT_PORT, 0),
        connection(blur_id, IMAGE_OUTPUT_PORT, merge_id, MERGE_IMAGES_PORT, 0),
        connection(
            merge_id,
            IMAGE_OUTPUT_PORT,
            output_node_id,
            IMAGE_INPUT_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled Line Image chain");
    let compiled = &plan.module_definitions[&fixture.definition_id];
    assert!(compiled.point_renderers.contains_key(&renderer_id));
    let frame = evaluate_render_plan_frame(&fixture.project, &plan, &plugins, 30, 1.0, None)
        .expect("evaluated Line Image chain");
    assert_eq!(point_scenes(&frame.items).len(), 1);
    assert!(has_group(&frame.items, blur_id, FrameGroupKind::Effect));
    assert!(has_group(&frame.items, merge_id, FrameGroupKind::Merge));
}

#[test]
fn store_name_validation_discovers_a_line_only_point_stream() {
    let (mut fixture, nodes) = point_fixture(1);
    let (connect_id, _) = use_line_endpoint(&mut fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    assert!(definition.graph.nodes.values().all(|node| {
        !matches!(
            node.content(),
            crate::model::NodeContent::NativeOperation(operation)
                if operation.catalog_id == PARTICLE_SPRITE_RENDERER_CATALOG_ID
        )
    }));

    let mut second = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Number).catalog_id(),
    )
    .unwrap();
    second.name = "density".to_string();
    let second_id = second.id;
    definition.graph.nodes.insert(second_id, second);
    definition.graph.connections.retain(|edge| {
        !(edge.from.node_id == nodes.store
            && edge.from.port == POINT_SOURCE_PORT
            && edge.to.node_id == connect_id
            && edge.to.port == POINT_SOURCE_PORT)
    });
    definition.graph.connections.extend([
        connection(
            nodes.store,
            POINT_SOURCE_PORT,
            second_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            second_id,
            POINT_SOURCE_PORT,
            connect_id,
            POINT_SOURCE_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;
    definition.validate().expect("valid Line-only Store chain");

    let error = validate_module_node_name(definition, second_id, "heat").unwrap_err();
    assert!(
        error.contains("display name") || error.contains("duplicate"),
        "{error}"
    );
    validate_module_node_name(definition, second_id, "temperature")
        .expect("unique Store name in Line stream");

    let renderer = definition.graph.nodes.values().find(|node| {
        matches!(
            node.content(),
            crate::model::NodeContent::NativeOperation(operation)
                if operation.catalog_id == POINT_LINE_RENDERER_CATALOG_ID
        )
    });
    assert!(renderer.is_some());
    assert_eq!(
        definition
            .graph
            .port_definition(
                &crate::model::authoring::ModulePortAddress {
                    node_id: connect_id,
                    port: POINT_CONNECTIONS_PORT.to_string(),
                },
                crate::model::project::PortDirection::Output,
            )
            .unwrap()
            .data_type,
        PortDataType::PointConnections
    );
}
