use std::collections::HashMap;

use super::compiler::compile_module;
use super::particle_tests::{
    ParticleFixture, connection, particle_fixture, particle_node_id, point_scenes,
};
use super::point_position_tests::detach_renderer_input;
use super::point_tests::{point_fixture, replace_fixture_source_with_grid};
use super::{CompiledPointRenderStyle, RenderCapability, RenderPlanCache, RenderPlanCompiler};
use crate::editor::{
    ModuleItemPlacement, PlexusNodeClipFactory, PlexusPublishedParameters, TimelineEditorService,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, MediaTime, ModuleDefinitionId,
    ModuleInstanceId, ModuleOutputId, RationalRate, SourceRef, TimelineInterval, TimelineItemId,
};
use crate::model::frame::point::{PointRenderStyle, PointSceneFrame, PointSceneSource};
use crate::model::node::{
    CONNECT_POINTS_CATALOG_ID, Node, NodeContent, PARTICLE_SPRITE_RENDERER_CATALOG_ID,
    POINT_CONNECTIONS_PORT, POINT_LINE_RENDERER_CATALOG_ID, POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::property::PropertyValue;
use crate::plugin::PluginManager;

struct PlexusFixture {
    project: AuthoringProject,
    definition_id: ModuleDefinitionId,
    output_id: ModuleOutputId,
    parameters: PlexusPublishedParameters,
    items: Vec<TimelineItemId>,
    instances: Vec<ModuleInstanceId>,
}

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).unwrap()
}

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(value.into())
}

fn plexus_fixture(count: usize) -> PlexusFixture {
    let project = AuthoringProject::new(
        "Plexus",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(20),
    )
    .unwrap();
    let track_id = *project.tracks.keys().next().unwrap();
    let service = TimelineEditorService::new(project).unwrap();
    let factory = PlexusNodeClipFactory::create("Shared Plexus").unwrap();
    let definition_id = factory.definition.id;
    service.add_module_definition(factory.definition).unwrap();
    let mut items = Vec::new();
    let mut instances = Vec::new();
    for index in 0..count {
        let (item, instance, _) = service
            .place_module_item(
                definition_id,
                ModuleItemPlacement {
                    track_id,
                    name: format!("Plexus {index}"),
                    output_id: factory.output_id,
                    interval: TimelineInterval::new(MediaTime::zero(), seconds(10)).unwrap(),
                    layer: index as i64,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .unwrap();
        items.push(item);
        instances.push(instance);
    }
    PlexusFixture {
        project: service.snapshot().unwrap().as_ref().clone(),
        definition_id,
        output_id: factory.output_id,
        parameters: factory.parameters,
        items,
        instances,
    }
}

fn sample(
    project: &AuthoringProject,
    frame: u64,
) -> Result<Vec<PointSceneFrame>, crate::error::LibraryError> {
    let plan =
        RenderPlanCompiler::compile(project).map_err(crate::error::LibraryError::Validation)?;
    let frame = super::evaluate_render_plan_frame(
        project,
        &plan,
        &PluginManager::default(),
        frame,
        1.0,
        None,
    )?;
    Ok(point_scenes(&frame.items).into_iter().cloned().collect())
}

/// Change only the endpoint in the established Particle/Point fixtures. Their
/// simulation, attribute SSA and Color Ramp branches remain production inputs.
pub(super) fn use_line_endpoint(fixture: &mut ParticleFixture) -> (uuid::Uuid, uuid::Uuid) {
    let renderer_id = particle_node_id(fixture, PARTICLE_SPRITE_RENDERER_CATALOG_ID);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = detach_renderer_input(definition, renderer_id);
    let connect = Node::new_catalog_node(CONNECT_POINTS_CATALOG_ID).unwrap();
    let connect_id = connect.id;
    let mut renderer = Node::new_catalog_node(POINT_LINE_RENDERER_CATALOG_ID).unwrap();
    renderer.id = renderer_id;
    definition
        .graph
        .nodes
        .extend([(connect_id, connect), (renderer_id, renderer)]);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            connect_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            connect_id,
            POINT_CONNECTIONS_PORT,
            renderer_id,
            POINT_CONNECTIONS_PORT,
            0,
        ),
    ]);
    definition.interface.parameters.retain(|parameter| {
        parameter.target.node_id != renderer_id || parameter.target.port == "color"
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    (connect_id, renderer_id)
}

#[test]
fn plexus_template_compiles_one_shared_definition_and_samples_grid_lines() {
    let fixture = plexus_fixture(100);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 100);
    let compiled = &plan.module_definitions[&fixture.definition_id];
    assert_eq!(
        fixture.project.module_definitions[&fixture.definition_id]
            .graph
            .nodes
            .len(),
        4
    );
    assert_eq!(
        compiled.nodes.len(),
        3,
        "Output is a compiled route, not an operation"
    );
    assert_eq!(compiled.point_renderers.len(), 1);
    assert!(compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu));
    assert!(matches!(
        compiled
            .point_renderers
            .values()
            .next()
            .unwrap()
            .render_style,
        CompiledPointRenderStyle::Lines { .. }
    ));
    let scenes = sample(&fixture.project, 15).unwrap();
    assert_eq!(scenes.len(), 100);
    for scene in scenes {
        let PointSceneSource::Grid(grid) = scene.source else {
            panic!("Grid source")
        };
        assert_eq!(grid.counts, [10, 7, 1]);
        let PointRenderStyle::Lines {
            connections,
            width,
            fade,
        } = scene.render_style
        else {
            panic!("Lines")
        };
        assert_eq!(connections.max_distance.0, 80.0);
        assert_eq!(connections.min_distance.0, 0.0);
        assert_eq!(connections.max_neighbors, 6);
        assert_eq!(width.0, 2.0);
        assert_eq!(fade.0, 0.0);
        assert!(scene.point_program.is_none());
    }
}

#[test]
fn line_controls_are_instance_local_do_not_recompile_definition_and_roundtrip() {
    let mut fixture = plexus_fixture(2);
    let original_definition = fixture.project.module_definitions[&fixture.definition_id].clone();
    let before = sample(&fixture.project, 15).unwrap();
    let mut cache = RenderPlanCache::default();
    let (first, _) = cache.compile(&fixture.project).unwrap();
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instances[0])
        .unwrap()
        .parameter_overrides
        .extend([
            (fixture.parameters.max_distance, number(120.0)),
            (fixture.parameters.width, number(5.0)),
            (fixture.parameters.fade, number(0.75)),
        ]);
    let (second, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert!(std::sync::Arc::ptr_eq(
        &first.module_definitions[&fixture.definition_id],
        &second.module_definitions[&fixture.definition_id]
    ));
    assert_eq!(
        fixture.project.module_definitions[&fixture.definition_id],
        original_definition
    );
    for scene in sample(&fixture.project, 15).unwrap() {
        let previous = before
            .iter()
            .find(|previous| previous.invocation == scene.invocation)
            .unwrap();
        assert_eq!(scene.source, previous.source);
        assert_eq!(scene.source_node_id, previous.source_node_id);
        if scene.invocation.module_instance_id == fixture.instances[0] {
            let PointRenderStyle::Lines {
                connections,
                width,
                fade,
            } = scene.render_style
            else {
                panic!("Lines")
            };
            assert_eq!(connections.max_distance.0, 120.0);
            assert_eq!(width.0, 5.0);
            assert_eq!(fade.0, 0.75);
        } else {
            assert_eq!(scene.render_style, previous.render_style);
        }
    }
    let encoded = serde_json::to_string(&fixture.project).unwrap();
    assert!(!encoded.contains("PointSceneFrame"));
    let decoded: AuthoringProject = serde_json::from_str(&encoded).unwrap();
    assert_eq!(fixture.project, decoded);
    assert_eq!(
        sample(&fixture.project, 15).unwrap(),
        sample(&decoded, 15).unwrap()
    );
}

#[test]
fn line_distance_and_width_keyframes_follow_local_time_after_clip_move() {
    let mut fixture = plexus_fixture(2);
    let item = fixture.project.items.get_mut(&fixture.items[0]).unwrap();
    item.interval = TimelineInterval::new(seconds(3), seconds(10)).unwrap();
    let SourceRef::Module(invocation) = &mut item.source else {
        panic!("Module")
    };
    for (parameter, start, end) in [
        (fixture.parameters.max_distance, 20.0, 100.0),
        (fixture.parameters.width, 2.0, 6.0),
    ] {
        invocation.automation_tracks.insert(
            parameter,
            AutomationTrack {
                keyframes: vec![
                    AutomationKeyframe::new(seconds(0), number(start), EasingFunction::Linear),
                    AutomationKeyframe::new(seconds(1), number(end), EasingFunction::Linear),
                ],
            },
        );
    }
    for (frame, distance, expected_width) in [(90, 20.0, 2.0), (105, 60.0, 4.0), (120, 100.0, 6.0)]
    {
        let scenes = sample(&fixture.project, frame).unwrap();
        let scene = scenes
            .iter()
            .find(|scene| scene.invocation.module_instance_id == fixture.instances[0])
            .unwrap();
        let PointRenderStyle::Lines {
            connections, width, ..
        } = &scene.render_style
        else {
            panic!("Lines")
        };
        assert_eq!(connections.max_distance.0, distance);
        assert_eq!(width.0, expected_width);
        let sibling = scenes
            .iter()
            .find(|scene| scene.invocation.module_instance_id == fixture.instances[1])
            .unwrap();
        let PointRenderStyle::Lines {
            connections, width, ..
        } = &sibling.render_style
        else {
            panic!("Lines")
        };
        assert_eq!(connections.max_distance.0, 80.0);
        assert_eq!(width.0, 2.0);
    }
}

#[test]
fn particle_and_grid_lines_reuse_custom_attributes_color_ramps_and_geometry_ssa() {
    for grid in [false, true] {
        let (mut fixture, nodes) = point_fixture(1);
        if grid {
            replace_fixture_source_with_grid(&mut fixture, &nodes, "random");
        }
        let (connect_id, renderer_id) = use_line_endpoint(&mut fixture);
        let definition = fixture
            .project
            .module_definitions
            .get_mut(&fixture.definition_id)
            .unwrap();
        let set = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
        let set_id = set.id;
        definition.graph.nodes.insert(set_id, set);
        let input = definition
            .graph
            .connections
            .iter_mut()
            .find(|connection| {
                connection.to.node_id == connect_id && connection.to.port == POINT_SOURCE_PORT
            })
            .unwrap();
        input.to.node_id = set_id;
        definition.graph.connections.push(connection(
            set_id,
            POINT_SOURCE_PORT,
            connect_id,
            POINT_SOURCE_PORT,
            0,
        ));
        definition.topology_revision += 1;
        let compiled = compile_module(definition).unwrap();
        let program = compiled.point_renderers[&renderer_id]
            .point_program
            .as_ref()
            .unwrap();
        assert_eq!(program.schema.attributes().len(), 1);
        assert!(program.position_register.is_some());
        assert!(program.sprite_selection_register.is_none());
        let scene = sample(&fixture.project, 15).unwrap().remove(0);
        assert_eq!(matches!(scene.source, PointSceneSource::Grid(_)), grid);
        assert!(matches!(scene.render_style, PointRenderStyle::Lines { .. }));
        assert!(
            scene
                .point_program
                .unwrap()
                .sprite_selection_register
                .is_none()
        );
    }
}

#[test]
fn varying_values_cannot_silently_drive_uniform_topology_or_line_controls() {
    for port in [
        "min_distance",
        "max_distance",
        "max_neighbors",
        "width",
        "fade",
    ] {
        let (mut fixture, nodes) = point_fixture(1);
        let (connect, renderer) = use_line_endpoint(&mut fixture);
        let definition = fixture
            .project
            .module_definitions
            .get_mut(&fixture.definition_id)
            .unwrap();
        definition.graph.connections.push(connection(
            nodes.info,
            "random",
            if matches!(port, "width" | "fade") {
                renderer
            } else {
                connect
            },
            port,
            0,
        ));
        let error = compile_module(definition).unwrap_err();
        assert!(
            error.contains("unsupported input")
                || error.contains("cannot connect Number to Integer"),
            "{port}: {error}"
        );
    }
}

#[test]
fn disabled_connection_endpoints_have_no_output_and_unsupported_bypass_is_rejected() {
    for catalog in [CONNECT_POINTS_CATALOG_ID, POINT_LINE_RENDERER_CATALOG_ID] {
        for bypass in [false, true] {
            let mut fixture = plexus_fixture(1);
            let definition = fixture
                .project
                .module_definitions
                .get_mut(&fixture.definition_id)
                .unwrap();
            let node = definition.graph.nodes.values_mut().find(|node| matches!(node.content(), NodeContent::NativeOperation(operation) if operation.catalog_id == catalog)).unwrap();
            if bypass {
                node.bypassed = true;
            } else {
                node.enabled = false;
            }
            definition.topology_revision += 1;
            if bypass {
                assert!(
                    compile_module(definition)
                        .unwrap_err()
                        .contains("cannot be bypassed")
                );
                continue;
            }
            let compiled = compile_module(definition).unwrap();
            assert!(compiled.point_renderers.is_empty());
            assert!(!compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu));
            assert!(
                sample(&fixture.project, 15).unwrap().is_empty(),
                "{catalog} bypass={bypass}"
            );
        }
    }
}

#[test]
fn invalid_published_topology_is_reported_instead_of_clamped() {
    for (minimum, maximum, neighbors) in [
        (81.0, 80.0, 6),
        (0.0, -1.0, 6),
        (0.0, 80.0, 0),
        (0.0, 80.0, 33),
    ] {
        let mut fixture = plexus_fixture(1);
        fixture
            .project
            .module_instances
            .get_mut(&fixture.instances[0])
            .unwrap()
            .parameter_overrides
            .extend([
                (fixture.parameters.min_distance, number(minimum)),
                (fixture.parameters.max_distance, number(maximum)),
                (
                    fixture.parameters.max_neighbors,
                    PropertyValue::Integer(neighbors),
                ),
            ]);
        assert!(sample(&fixture.project, 0).is_err());
    }
    let mut particle = particle_fixture(1);
    use_line_endpoint(&mut particle);
    assert!(matches!(
        sample(&particle.project, 15).unwrap()[0].source,
        PointSceneSource::Particle { .. }
    ));
}
