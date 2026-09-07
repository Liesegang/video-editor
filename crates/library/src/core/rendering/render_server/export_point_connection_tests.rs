#![cfg(all(feature = "gl", target_os = "windows"))]

use super::*;
use crate::model::authoring::{ModuleConnection, ModuleConnectionId, ModulePortAddress};
use crate::model::node::{
    CONNECT_POINTS_CATALOG_ID, Node, NodeContent, PARTICLE_SPRITE_RENDERER_CATALOG_ID,
    PARTICLE_SYSTEM_PORT, POINT_CONNECTIONS_PORT, POINT_LINE_RENDERER_CATALOG_ID,
    POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::property::{Property, PropertyValue, Vec3};
use crate::plugin::{DecodedPixelBuffer, LoadRequest};
use ordered_float::OrderedFloat;
use std::collections::HashSet;

const SENTINEL: &[u8] = b"keep this existing Plexus export";

fn connection(
    from_node: uuid::Uuid,
    from_port: &str,
    to_node: uuid::Uuid,
    to_port: &str,
) -> ModuleConnection {
    ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: from_node,
            port: from_port.to_string(),
        },
        to: ModulePortAddress {
            node_id: to_node,
            port: to_port.to_string(),
        },
        order: 0,
        blend_mode: crate::model::BlendMode::Normal,
    }
}

/// Replace only the Sprite endpoint in an established production fixture.
/// The upstream Particle/Grid source and every field instruction remain the
/// same. Reusing the renderer UUID deliberately keeps the existing published
/// Color parameter and its Instance override authoritative for Line Renderer.
fn with_line_endpoint(project: Arc<AuthoringProject>) -> Arc<AuthoringProject> {
    let mut project = project.as_ref().clone();
    let definition_id = *project.module_definitions.keys().next().unwrap();
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let renderer_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(content)
                    if content.catalog_id == PARTICLE_SPRITE_RENDERER_CATALOG_ID
            )
        })
        .expect("the production fixture must contain its Sprite endpoint")
        .id;
    let upstream = definition
        .graph
        .connections
        .iter()
        .find(|candidate| {
            candidate.to.node_id == renderer_id && candidate.to.port == PARTICLE_SYSTEM_PORT
        })
        .expect("the Sprite endpoint must have one Point source")
        .from
        .clone();
    definition.graph.connections.retain(|candidate| {
        !(candidate.to.node_id == renderer_id && candidate.to.port == PARTICLE_SYSTEM_PORT)
    });
    definition.graph.nodes.remove(&renderer_id).unwrap();

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
        ),
        connection(
            connect_id,
            POINT_CONNECTIONS_PORT,
            renderer_id,
            POINT_CONNECTIONS_PORT,
        ),
    ]);

    definition.interface.parameters.retain(|parameter| {
        parameter.target.node_id != renderer_id || parameter.target.port == "color"
    });
    let surviving_parameters = definition
        .interface
        .parameters
        .iter()
        .map(|parameter| parameter.id)
        .collect::<HashSet<_>>();
    definition.topology_revision += 1;
    definition.interface_version += 1;
    for instance in project
        .module_instances
        .values_mut()
        .filter(|instance| instance.definition_id == definition_id)
    {
        instance
            .parameter_overrides
            .retain(|id, _| surviving_parameters.contains(id));
    }
    project.validate().unwrap();
    Arc::new(project)
}

fn grid_line_project() -> Arc<AuthoringProject> {
    with_line_endpoint(point_tests::grid_export_project())
}

fn particle_line_project() -> Arc<AuthoringProject> {
    with_line_endpoint(particle_export_project())
}

fn one_frame_project(project: Arc<AuthoringProject>) -> Arc<AuthoringProject> {
    let mut project = project.as_ref().clone();
    let duration = MediaTime::new(1, 30).unwrap();
    project
        .timelines
        .get_mut(&project.root_timeline_id)
        .unwrap()
        .duration = duration;
    project
        .timelines
        .get_mut(&project.root_timeline_id)
        .unwrap()
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    for item in project.items.values_mut() {
        item.interval = TimelineInterval::new(MediaTime::zero(), duration).unwrap();
    }
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn grid_plexus_png_export_matches_preview_and_contains_lines() {
    let pixels = assert_authoring_png_matches_preview(
        RenderServer::new(
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        ),
        grid_line_project(),
        30,
    );
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 16 && pixel[..3].iter().any(|channel| *channel > 32)),
        "Grid Plexus export must contain a visible line pixel"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn particle_plexus_png_export_matches_preview_and_contains_lines() {
    let pixels = assert_authoring_png_matches_preview(
        RenderServer::new(
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        ),
        particle_line_project(),
        60,
    );
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 16 && pixel[..3].iter().any(|channel| *channel > 32)),
        "Particle Plexus export must contain a visible line pixel"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU and bundled FFmpeg"]
fn plexus_video_export_decodes_a_line_pixel_from_real_ffmpeg_output() {
    let project = one_frame_project(grid_line_project());
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let plugins = Arc::new(PluginManager::default());
    let cache = Arc::new(CacheManager::new());
    let server = RenderServer::new(Arc::clone(&plugins), Arc::clone(&cache));
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("plexus.mp4");

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(995),
        Arc::clone(&project),
        plan,
        project.root_timeline_id,
        output.to_string_lossy().into_owned(),
    ));
    let result = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    result.output.unwrap();
    assert_eq!(result.frames_exported, 1);
    assert!(result.published);

    let decoded = plugins
        .load_resource(
            &LoadRequest::VideoFrame {
                path: output.to_string_lossy().into_owned(),
                source_time: 0.0,
                stream_index: None,
                source_color_authority: None,
            },
            cache.as_ref(),
        )
        .unwrap();
    let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded.pixels() else {
        panic!("FFmpeg video decode must preserve its typed RGBAF32 output")
    };
    let line_pixels = decoded
        .data()
        .iter()
        .filter(|pixel| pixel[0].max(pixel[1]).max(pixel[2]) > 0.125)
        .count();
    assert!(
        line_pixels > 0,
        "decoded video frame must contain a visible Plexus line"
    );
}

fn dense_grid_line_project() -> Arc<AuthoringProject> {
    let mut project = one_frame_project(grid_line_project()).as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let grid = definition
        .graph
        .nodes
        .values_mut()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(content)
                    if content.catalog_id == PointNodeRole::Grid.catalog_id()
            )
        })
        .unwrap();
    for (key, value) in [
        ("count_x", PropertyValue::Integer(65)),
        ("count_y", PropertyValue::Integer(65)),
        ("count_z", PropertyValue::Integer(1)),
    ] {
        grid.set_property(key.to_string(), Property::constant(value))
            .unwrap();
    }
    grid.set_property(
        "spacing".to_string(),
        Property::constant(PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
            z: OrderedFloat(0.0),
        })),
    )
    .unwrap();
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn dense_plexus_budget_failure_preserves_output_and_removes_staging_file() {
    let project = dense_grid_line_project();
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let server = RenderServer::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("dense-plexus.mp4");
    fs::write(&output, SENTINEL).unwrap();

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(996),
        Arc::clone(&project),
        plan,
        project.root_timeline_id,
        output.to_string_lossy().into_owned(),
    ));
    let result = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let error = result.output.unwrap_err().to_string();
    assert!(error.contains("Point proximity requires"), "{error}");
    assert!(error.contains("candidate tests"), "{error}");
    assert_eq!(result.frames_exported, 0);
    assert!(!result.published);
    assert_eq!(fs::read(&output).unwrap(), SENTINEL);
    let entries = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        [output],
        "failed Plexus export must remove its sibling staging artifact"
    );
}
