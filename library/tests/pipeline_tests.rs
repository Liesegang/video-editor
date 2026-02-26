//! Integration tests for the pull-based rendering pipeline (EvalEngine).
//!
//! These tests verify that the full evaluation pipeline works end-to-end:
//! project setup → graph node creation → EvalEngine evaluation.

use std::sync::{Arc, RwLock};

use library::pipeline::engine::EvalEngine;
use library::plugin::PluginManager;
use library::project::node::Node;
use library::project::project::{Composition, Project};
use library::project::track::TrackData;
use library::rendering::cache::CacheManager;
use library::rendering::renderer::RenderOutput;
use library::rendering::skia_renderer::SkiaRenderer;
use library::runtime::color::Color;
use library::service::handlers::layer_factory::LayerFactory;
use library::service::handlers::source_handler::SourceHandler;
use library::service::handlers::track_handler::TrackHandler;

fn make_plugin_manager() -> PluginManager {
    PluginManager::default()
}

fn setup_project() -> (Arc<RwLock<Project>>, uuid::Uuid, uuid::Uuid) {
    let mut project = Project::new("Test Project");
    let comp = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
    let comp_id = comp.id;
    project.add_composition(comp);
    let root_track = TrackData::new("Root");
    let root_track_id = root_track.id;
    project.add_node(Node::Track(root_track));
    project
        .get_composition_mut(comp_id)
        .unwrap()
        .child_ids
        .push(root_track_id);
    (Arc::new(RwLock::new(project)), comp_id, root_track_id)
}

fn make_renderer() -> SkiaRenderer {
    let bg = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    SkiaRenderer::new(1920, 1080, bg, false, None)
}

/// Test: a text clip with fill + transform should produce an image through the pipeline.
#[test]
fn test_text_clip_renders_through_pipeline() {
    let _ = env_logger::builder().is_test(true).try_init();
    let (project, comp_id, _root_track_id) = setup_project();
    let plugin_manager = make_plugin_manager();

    // Add track + text clip with graph nodes
    let track_id = TrackHandler::add_track(&project, comp_id, "Track 1").unwrap();
    let text_clip = LayerFactory::build_text_source("Hello World", 0, 90, 30.0);
    let clip_kind = text_clip.kind.clone();
    let clip_id =
        SourceHandler::add_source_to_track(&project, comp_id, track_id, text_clip, 0, 90, None)
            .unwrap();
    SourceHandler::setup_source_graph_nodes(
        &project,
        &plugin_manager,
        track_id,
        clip_id,
        &clip_kind,
    )
    .unwrap();

    // Verify the project state
    let proj = project.read().unwrap();
    println!("Nodes: {}", proj.nodes.len());
    println!("Connections: {}", proj.connections.len());
    for conn in &proj.connections {
        println!(
            "  {} {} -> {} {}",
            conn.from.node_id, conn.from.pin_name, conn.to.node_id, conn.to.pin_name
        );
    }

    // Check root track -> track -> layer structure
    let track = proj.get_track(track_id).unwrap();
    println!("Track {} children: {:?}", track_id, track.child_ids);
    assert_eq!(
        track.child_ids.len(),
        1,
        "Track should have 1 child (layer)"
    );
    let layer_id = track.child_ids[0];
    let layer = proj.get_layer(layer_id).expect("Layer should exist");
    println!("Layer {} children: {:?}", layer_id, layer.child_ids);

    // Check that the composition has the child track
    let comp = proj.get_composition(comp_id).unwrap();
    println!("Composition {} children: {:?}", comp_id, comp.child_ids);
    assert!(
        comp.child_ids.contains(&track_id),
        "Composition should contain the child track"
    );

    // Now run the evaluation engine
    let engine = EvalEngine::with_default_evaluators();
    let mut renderer = make_renderer();
    let cache_manager = CacheManager::new();
    let property_evaluators = plugin_manager.get_property_evaluators();

    let result = engine.evaluate_composition(
        &proj,
        comp,
        &plugin_manager,
        &mut renderer,
        &cache_manager,
        property_evaluators,
        0, // frame_number = 0 (within clip range [0..90])
        1.0,
        None,
    );

    match &result {
        Ok(output) => {
            println!("Render succeeded: {:?}", std::mem::discriminant(output));
            match output {
                RenderOutput::Image(img) => {
                    println!("Image size: {}x{}", img.width, img.height);
                    // The image should not be completely black (text should be rendered)
                    let has_non_zero = img.data.iter().any(|&b| b != 0);
                    assert!(
                        has_non_zero,
                        "Rendered image should contain non-zero pixels (text should be visible)"
                    );
                }
                RenderOutput::Texture(_) => {
                    println!("Got texture output (expected in GPU mode)");
                }
            }
        }
        Err(e) => {
            panic!("Render failed: {}", e);
        }
    }
}

/// Test: a shape clip with fill + transform should produce an image.
#[test]
fn test_shape_clip_renders_through_pipeline() {
    let (project, comp_id, _) = setup_project();
    let plugin_manager = make_plugin_manager();

    let track_id = TrackHandler::add_track(&project, comp_id, "Track 1").unwrap();
    let shape_clip = LayerFactory::build_shape_source(0, 90, 30.0);
    let clip_kind = shape_clip.kind.clone();
    let clip_id =
        SourceHandler::add_source_to_track(&project, comp_id, track_id, shape_clip, 0, 90, None)
            .unwrap();
    SourceHandler::setup_source_graph_nodes(
        &project,
        &plugin_manager,
        track_id,
        clip_id,
        &clip_kind,
    )
    .unwrap();

    let proj = project.read().unwrap();
    let comp = proj.get_composition(comp_id).unwrap();

    let engine = EvalEngine::with_default_evaluators();
    let mut renderer = make_renderer();
    let cache_manager = CacheManager::new();
    let property_evaluators = plugin_manager.get_property_evaluators();

    let result = engine.evaluate_composition(
        &proj,
        comp,
        &plugin_manager,
        &mut renderer,
        &cache_manager,
        property_evaluators,
        0,
        1.0,
        None,
    );

    match &result {
        Ok(RenderOutput::Image(img)) => {
            println!("Shape render: {}x{}", img.width, img.height);
            let has_non_zero = img.data.iter().any(|&b| b != 0);
            assert!(has_non_zero, "Shape render should produce non-zero pixels");
        }
        Ok(_) => println!("Got non-image output"),
        Err(e) => panic!("Shape render failed: {}", e),
    }
}

/// Test: an SkSL clip should produce an image through the pipeline.
#[test]
fn test_sksl_clip_renders_through_pipeline() {
    let (project, comp_id, _) = setup_project();
    let plugin_manager = make_plugin_manager();

    let track_id = TrackHandler::add_track(&project, comp_id, "Track 1").unwrap();
    let sksl_clip = LayerFactory::build_sksl_source(0, 90, 30.0);
    let clip_kind = sksl_clip.kind.clone();
    let clip_id =
        SourceHandler::add_source_to_track(&project, comp_id, track_id, sksl_clip, 0, 90, None)
            .unwrap();
    SourceHandler::setup_source_graph_nodes(
        &project,
        &plugin_manager,
        track_id,
        clip_id,
        &clip_kind,
    )
    .unwrap();

    let proj = project.read().unwrap();
    let comp = proj.get_composition(comp_id).unwrap();

    let engine = EvalEngine::with_default_evaluators();
    let mut renderer = make_renderer();
    let cache_manager = CacheManager::new();
    let property_evaluators = plugin_manager.get_property_evaluators();

    let result = engine.evaluate_composition(
        &proj,
        comp,
        &plugin_manager,
        &mut renderer,
        &cache_manager,
        property_evaluators,
        0,
        1.0,
        None,
    );

    // SkSL may fail without GPU context, so just check it doesn't panic
    match &result {
        Ok(RenderOutput::Image(img)) => {
            println!("SkSL render: {}x{}", img.width, img.height);
        }
        Ok(_) => println!("Got non-image output"),
        Err(e) => {
            println!("SkSL render failed (may need GPU): {}", e);
            // SkSL may fail without GPU, that's acceptable in CI
        }
    }
}

/// Test: verify basic Skia rendering works (sanity check).
#[test]
fn test_basic_skia_rendering_works() {
    use library::rendering::renderer::Renderer;
    use library::runtime::draw_type::DrawStyle;
    use library::runtime::entity::StyleConfig;
    use library::runtime::transform::Transform;

    let mut renderer = make_renderer();
    renderer.clear().unwrap();

    // Try to render a simple shape directly
    let style = StyleConfig {
        id: uuid::Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            offset: 0.0,
        },
    };

    let result = renderer.rasterize_shape_layer(
        "M 100,100 L 200,100 L 200,200 L 100,200 Z",
        &[style],
        &vec![],
        &Transform::default(),
    );

    match &result {
        Ok(RenderOutput::Image(img)) => {
            println!("Basic shape render: {}x{}", img.width, img.height);
            let has_non_zero = img.data.iter().any(|&b| b != 0);
            println!("Has non-zero pixels: {}", has_non_zero);
            assert!(has_non_zero, "Direct shape render should produce pixels");
        }
        Ok(_) => panic!("Expected Image output"),
        Err(e) => panic!("Basic shape render failed: {}", e),
    }

    // Now draw the result onto the main surface and finalize
    let shape_img = result.unwrap();
    renderer
        .draw_layer(&shape_img, &Transform::default())
        .unwrap();
    let output = renderer.finalize().unwrap();
    match &output {
        RenderOutput::Image(img) => {
            let has_non_zero = img.data.iter().any(|&b| b != 0);
            println!("Finalized has non-zero: {}", has_non_zero);
            assert!(has_non_zero, "Finalized surface should have pixels");
        }
        _ => panic!("Expected Image output from finalize"),
    }
}

/// Test: check what the TransformEvaluator does with default properties (scale 100%).
#[test]
fn test_transform_scale_issue() {
    use library::rendering::renderer::Renderer;
    use library::runtime::draw_type::DrawStyle;
    use library::runtime::entity::StyleConfig;
    use library::runtime::transform::{Position, Scale, Transform};

    let mut renderer = make_renderer();
    renderer.clear().unwrap();

    // First create a simple red square image
    let style = StyleConfig {
        id: uuid::Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            offset: 0.0,
        },
    };
    let shape_img = renderer
        .rasterize_shape_layer(
            "M 100,100 L 200,100 L 200,200 L 100,200 Z",
            &[style],
            &vec![],
            &Transform::default(),
        )
        .unwrap();

    // Now apply transform with 100% scale (as the TransformEvaluator would)
    let transform_100 = Transform {
        position: Position { x: 0.0, y: 0.0 },
        anchor: Position { x: 0.0, y: 0.0 },
        scale: Scale { x: 100.0, y: 100.0 }, // This is what TransformEvaluator defaults to!
        rotation: 0.0,
        opacity: 1.0,
    };

    let transformed = renderer
        .transform_layer(&shape_img, &transform_100)
        .unwrap();
    match &transformed {
        RenderOutput::Image(img) => {
            let has_non_zero = img.data.iter().any(|&b| b != 0);
            println!("Transform with scale 100.0: has_non_zero={}", has_non_zero);
            // Scale 100.0 means 100x scaling in Skia — the image is zoomed 100x!
            // Only a tiny corner of the original would be visible
        }
        _ => panic!("Expected Image"),
    }

    // Now test with scale 1.0 (which is what Scale::default() uses)
    let transform_1 = Transform {
        position: Position { x: 0.0, y: 0.0 },
        anchor: Position { x: 0.0, y: 0.0 },
        scale: Scale { x: 1.0, y: 1.0 },
        rotation: 0.0,
        opacity: 1.0,
    };

    let transformed_correct = renderer.transform_layer(&shape_img, &transform_1).unwrap();
    match &transformed_correct {
        RenderOutput::Image(img) => {
            let has_non_zero = img.data.iter().any(|&b| b != 0);
            println!("Transform with scale 1.0: has_non_zero={}", has_non_zero);
            assert!(has_non_zero, "Scale 1.0 transform should preserve pixels");
        }
        _ => panic!("Expected Image"),
    }
}

/// Test: verify the project structure matches what the render server would receive.
#[test]
fn test_project_structure_for_render_server() {
    let (project, comp_id, _root_track_id) = setup_project();
    let plugin_manager = make_plugin_manager();

    let track_id = TrackHandler::add_track(&project, comp_id, "Track 1").unwrap();
    let text_clip = LayerFactory::build_text_source("Test", 0, 30, 30.0);
    let clip_kind = text_clip.kind.clone();
    let clip_id =
        SourceHandler::add_source_to_track(&project, comp_id, track_id, text_clip, 0, 30, None)
            .unwrap();
    SourceHandler::setup_source_graph_nodes(
        &project,
        &plugin_manager,
        track_id,
        clip_id,
        &clip_kind,
    )
    .unwrap();

    // Clone the project (simulating what the render server receives)
    let proj_clone = project.read().unwrap().clone();

    // Verify the cloned project has all nodes and connections
    assert!(
        proj_clone.nodes.len() >= 5,
        "Cloned project should have at least 5 nodes (root track + track + layer + clip + 2 graph nodes), got {}",
        proj_clone.nodes.len()
    );
    assert!(
        proj_clone.connections.len() >= 3,
        "Cloned project should have at least 3 connections, got {}",
        proj_clone.connections.len()
    );

    // Verify the composition exists in the clone and has children
    let comp = proj_clone.get_composition(comp_id).unwrap();
    assert!(
        comp.child_ids.len() >= 2,
        "Composition should have at least 2 children (root track + added track) in cloned project"
    );

    // Walk the tree: root -> track -> layer -> (clip + graph nodes)
    let track = proj_clone.get_track(track_id).unwrap();
    assert_eq!(
        track.child_ids.len(),
        1,
        "Track should have 1 child (layer)"
    );
    let layer_id = track.child_ids[0];
    let layer = proj_clone.get_layer(layer_id).unwrap();
    println!(
        "Cloned layer {} has {} children",
        layer_id,
        layer.child_ids.len()
    );

    // Verify connections are in the clone
    for conn in &proj_clone.connections {
        println!(
            "  Clone conn: {}.{} -> {}.{}",
            conn.from.node_id, conn.from.pin_name, conn.to.node_id, conn.to.pin_name
        );
    }

    // Verify find_upstream works (this is what evaluate_track uses)
    let upstream = proj_clone
        .connections
        .iter()
        .find(|c| c.to.node_id == layer_id && c.to.pin_name == "image_out");
    assert!(
        upstream.is_some(),
        "Layer should have upstream connection for image_out (transform.image_out → layer.image_out)"
    );
    println!(
        "Layer upstream: {}.{}",
        upstream.unwrap().from.node_id,
        upstream.unwrap().from.pin_name
    );
}
