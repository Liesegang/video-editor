use super::point_support::{test_gradient, test_random};
use super::*;
use crate::model::frame::point::{
    PointConnectionParameters, PointGridParameters, PointRenderStyle,
};
use crate::model::point::{
    NumericBinaryOperation, PointAttributeSchema, PointInstruction, PointRenderProgram,
};
use crate::model::property::{ColorValue, GradientSpread, PropertyValue};
use crate::rendering::scene_runtime::{PointConnectionReadback, PointInvocationStats};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub(super) fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

pub(super) fn line_style(
    maximum: f32,
    max_neighbors: u32,
    width: f32,
    fade: f32,
) -> PointRenderStyle {
    PointRenderStyle::Lines {
        connections: PointConnectionParameters {
            min_distance: 0.0.into(),
            max_distance: maximum.into(),
            max_neighbors,
        },
        width: width.into(),
        fade: fade.into(),
    }
}

pub(super) fn grid_scene(counts: [u32; 3], spacing: [f64; 3]) -> PointSceneFrame {
    let mut scene = particle_scene(0);
    scene.source = PointSceneSource::Grid(PointGridParameters {
        counts,
        spacing: particle_vec3(spacing[0], spacing[1], spacing[2]),
        center: particle_vec3(0.0, 0.0, 0.0),
        size: 4.0.into(),
        seed: 123,
    });
    scene.color = Color::white();
    scene.render_style = line_style(21.0, 2, 2.0, 0.0);
    scene.point_program = None;
    scene
}

pub(super) fn render(renderer: &mut SkiaRenderer, scene: &PointSceneFrame) -> Image {
    render_point_test_scene(renderer, scene).unwrap()
}

pub(super) fn connections(
    renderer: &SkiaRenderer,
    scene: &PointSceneFrame,
) -> Vec<PointConnectionReadback> {
    let mut connections = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_connections(scene)
        .unwrap();
    connections.sort_by_key(|edge| (edge.source_serial, edge.target_serial));
    connections
}

pub(super) fn edge_pairs(edges: &[PointConnectionReadback]) -> Vec<(u32, u32)> {
    edges
        .iter()
        .map(|edge| {
            assert!(edge.source_serial < edge.target_serial);
            (edge.source_serial, edge.target_serial)
        })
        .collect()
}

pub(super) fn stats(renderer: &SkiaRenderer, scene: &PointSceneFrame) -> PointInvocationStats {
    renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .invocation_stats(&scene.invocation)
        .unwrap()
}

fn degree(edges: &[PointConnectionReadback]) -> BTreeMap<u32, usize> {
    let mut degrees = BTreeMap::new();
    for edge in edges {
        *degrees.entry(edge.source_serial).or_default() += 1;
        *degrees.entry(edge.target_serial).or_default() += 1;
    }
    degrees
}

fn alpha_sum(image: &Image) -> u64 {
    image
        .data
        .chunks_exact(4)
        .map(|pixel| u64::from(pixel[3]))
        .sum()
}

fn nontransparent_pixels(image: &Image) -> usize {
    image
        .data
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .count()
}

fn assert_simulation_same(before: &PointInvocationStats, after: &PointInvocationStats) {
    assert_eq!(after.simulation_generation, before.simulation_generation);
    assert_eq!(after.simulated_steps, before.simulated_steps);
    assert_eq!(after.checkpoint_restores, before.checkpoint_restores);
    assert_eq!(after.current_step, before.current_step);
    assert_eq!(after.checkpoint_steps, before.checkpoint_steps);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn grid_connections_are_exact_mutual_nearest_with_stable_ties_and_3d_distance() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let scene = grid_scene([3, 2, 1], [20.0, 20.0, 20.0]);
    render(&mut renderer, &scene);
    let edges = connections(&renderer, &scene);
    assert_eq!(
        edge_pairs(&edges),
        vec![(0, 1), (0, 1024), (1, 2), (2, 1026), (1024, 1025)]
    );
    assert_eq!(
        degree(&edges),
        BTreeMap::from([(0, 2), (1, 2), (2, 2), (1024, 2), (1025, 1), (1026, 1)])
    );
    assert!(
        edges
            .iter()
            .all(|edge| (edge.distance - 20.0).abs() <= 1e-5)
    );
    let observed = stats(&renderer, &scene);
    assert_eq!(observed.compact_edge_count, edges.len() as u32);
    assert!(observed.max_point_candidates <= scene.source.capacity());
    assert!(observed.candidate_tests >= edges.len() as u32 * 2);

    let mut cube = grid_scene([2, 2, 2], [10.0, 10.0, 10.0]);
    cube.render_style = line_style(11.0, 32, 2.0, 0.0);
    render(&mut renderer, &cube);
    let cube_edges = connections(&renderer, &cube);
    assert_eq!(
        edge_pairs(&cube_edges),
        vec![
            (0, 1),
            (0, 1024),
            (0, 1_048_576),
            (1, 1025),
            (1, 1_048_577),
            (1024, 1025),
            (1024, 1_049_600),
            (1025, 1_049_601),
            (1_048_576, 1_048_577),
            (1_048_576, 1_049_600),
            (1_048_577, 1_049_601),
            (1_049_600, 1_049_601),
        ],
        "a 2x2x2 cube must connect its twelve exact 3D axis edges"
    );
    assert!(
        cube_edges
            .iter()
            .all(|edge| (edge.distance - 10.0).abs() <= 1e-5)
    );
    assert!(degree(&cube_edges).values().all(|value| *value == 3));
}

pub(super) fn scaled_position_program(scale_x: f64) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Position,
            PointInstruction::Constant {
                value: PropertyValue::Vec3(particle_vec3(scale_x, 1.0, 1.0)),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Multiply,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant {
                value: PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white())),
            },
        ],
        ramps: Vec::new(),
        color_register: 3,
        position_register: Some(2),
        size_register: None,
        sprite_selection_register: None,
    }
}

fn ramp_program() -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Random { channel: 0 },
            PointInstruction::ColorRamp {
                gradient: 0,
                factor: 0,
            },
        ],
        ramps: vec![test_gradient(
            GradientSpread::Pad,
            &[
                (
                    0.0,
                    Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                ),
                (
                    1.0,
                    Color {
                        r: 0,
                        g: 0,
                        b: 255,
                        a: 255,
                    },
                ),
            ],
        )],
        color_register: 1,
        position_register: None,
        size_register: None,
        sprite_selection_register: None,
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn lines_consume_post_position_and_color_ramp_fields_then_apply_width_and_fade() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = grid_scene([3, 2, 1], [20.0, 20.0, 20.0]);
    let baseline = render(&mut renderer, &scene);
    let baseline_edges = connections(&renderer, &scene);
    let baseline_stats = stats(&renderer, &scene);

    scene.point_program = Some(scaled_position_program(2.0));
    let positioned = render(&mut renderer, &scene);
    let positioned_edges = connections(&renderer, &scene);
    assert_eq!(
        edge_pairs(&positioned_edges),
        vec![(0, 1024), (1, 1025), (2, 1026)],
        "Connect Points must consume Set Position's derived geometry"
    );
    assert_ne!(positioned.data, baseline.data);

    scene.point_program = Some(ramp_program());
    let narrow = render(&mut renderer, &scene);
    let ramp_edges = connections(&renderer, &scene);
    let narrow_stats = stats(&renderer, &scene);
    assert_eq!(edge_pairs(&ramp_edges), edge_pairs(&baseline_edges));
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    let seed = crate::rendering::scene_runtime::invocation_seed(&scene);
    for field in &fields {
        let expected = crate::color_management::sample_gradient_at(
            &scene.point_program.as_ref().unwrap().ramps[0],
            f64::from(test_random(seed, field.serial, 0)),
        )
        .unwrap()
        .rgba();
        for (actual, expected) in field.color.iter().zip(expected) {
            assert!((f64::from(*actual) - expected).abs() <= 2e-6);
        }
    }
    assert!(
        narrow
            .data
            .chunks_exact(4)
            .any(|pixel| u16::from(pixel[0]) > u16::from(pixel[2]) + 16)
    );
    assert!(
        narrow
            .data
            .chunks_exact(4)
            .any(|pixel| u16::from(pixel[2]) > u16::from(pixel[0]) + 16)
    );

    scene.render_style = line_style(21.0, 2, 8.0, 0.0);
    let wide = render(&mut renderer, &scene);
    let wide_stats = stats(&renderer, &scene);
    assert!(nontransparent_pixels(&wide) > nontransparent_pixels(&narrow) * 2);
    assert_eq!(
        wide_stats.connection_generation,
        narrow_stats.connection_generation
    );
    assert_eq!(
        edge_pairs(&connections(&renderer, &scene)),
        edge_pairs(&ramp_edges)
    );

    scene.render_style = line_style(21.0, 2, 8.0, 1.0);
    let faded = render(&mut renderer, &scene);
    assert!(alpha_sum(&faded) < alpha_sum(&wide) / 2);
    let faded_stats = stats(&renderer, &scene);
    assert_eq!(
        faded_stats.connection_generation,
        wide_stats.connection_generation
    );
    assert_eq!(faded_stats.connection_bytes, wide_stats.connection_bytes);
    assert_eq!(
        faded_stats.compact_edge_count,
        wide_stats.compact_edge_count
    );
    assert_eq!(baseline_stats.simulation_generation, 0);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn compact_scan_crosses_workgroup_boundaries_and_honors_degree_extremes() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    for count in [255_u32, 256, 257] {
        let mut scene = grid_scene([count, 1, 1], [10.0, 10.0, 10.0]);
        scene.render_style = line_style(10.1, 2, 1.0, 0.0);
        render(&mut renderer, &scene);
        let edges = connections(&renderer, &scene);
        assert_eq!(edges.len(), count as usize - 1);
        assert_eq!(
            edge_pairs(&edges),
            (0..count - 1)
                .map(|serial| (serial, serial + 1))
                .collect::<Vec<_>>()
        );
        assert_eq!(stats(&renderer, &scene).compact_edge_count, count - 1);
    }

    let mut one_neighbor = grid_scene([5, 1, 1], [10.0, 10.0, 10.0]);
    one_neighbor.render_style = line_style(10.1, 1, 1.0, 0.0);
    render(&mut renderer, &one_neighbor);
    let one_edges = connections(&renderer, &one_neighbor);
    assert_eq!(edge_pairs(&one_edges), vec![(0, 1)]);
    assert!(degree(&one_edges).values().all(|value| *value <= 1));

    let mut thirty_two = grid_scene([33, 1, 1], [1.0, 1.0, 1.0]);
    thirty_two.render_style = line_style(33.0, 32, 1.0, 0.0);
    render(&mut renderer, &thirty_two);
    let all_edges = connections(&renderer, &thirty_two);
    assert_eq!(all_edges.len(), 33 * 32 / 2);
    assert!(degree(&all_edges).values().all(|value| *value == 32));

    thirty_two.render_style = line_style(0.0, 32, 1.0, 0.0);
    render(&mut renderer, &thirty_two);
    assert!(connections(&renderer, &thirty_two).is_empty());
    assert_eq!(stats(&renderer, &thirty_two).compact_edge_count, 0);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn particle_seek_is_deterministic_and_line_topology_or_appearance_never_resets_simulation() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.color = Color::white();
    scene.render_style = line_style(80.0, 6, 2.0, 0.0);
    render(&mut renderer, &scene);
    let warm_edges = connections(&renderer, &scene);
    assert!(!warm_edges.is_empty());
    let warm = stats(&renderer, &scene);

    scene.color = Color {
        r: 255,
        g: 64,
        b: 32,
        a: 255,
    };
    scene.render_style = line_style(80.0, 6, 7.0, 1.0);
    render(&mut renderer, &scene);
    let appearance = stats(&renderer, &scene);
    assert_simulation_same(&warm, &appearance);
    assert_eq!(appearance.connection_generation, warm.connection_generation);
    assert_eq!(
        edge_pairs(&connections(&renderer, &scene)),
        edge_pairs(&warm_edges)
    );

    scene.render_style = line_style(80.0, 3, 7.0, 1.0);
    render(&mut renderer, &scene);
    let topology = stats(&renderer, &scene);
    assert_simulation_same(&warm, &topology);
    assert!(topology.connection_generation > appearance.connection_generation);
    assert!(topology.connection_bytes < appearance.connection_bytes);
    let topology_edges = connections(&renderer, &scene);
    assert!(degree(&topology_edges).values().all(|degree| *degree <= 3));

    if let PointSceneSource::Particle { target_step, .. } = &mut scene.source {
        *target_step = 300;
    } else {
        panic!("Particle fixture");
    }
    render(&mut renderer, &scene);
    if let PointSceneSource::Particle { target_step, .. } = &mut scene.source {
        *target_step = 480;
    } else {
        panic!("Particle fixture");
    }
    render(&mut renderer, &scene);
    assert_eq!(
        edge_pairs(&connections(&renderer, &scene)),
        edge_pairs(&topology_edges)
    );
    let replayed = stats(&renderer, &scene);
    assert_eq!(replayed.simulation_generation, warm.simulation_generation);
    assert_eq!(replayed.current_step, 480);
    assert!(replayed.checkpoint_restores > warm.checkpoint_restores);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn rejected_line_bounds_leave_the_valid_invocation_recoverable() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let scene = grid_scene([3, 2, 1], [20.0, 20.0, 20.0]);
    let baseline = render(&mut renderer, &scene);
    let baseline_edges = connections(&renderer, &scene);
    let baseline_stats = stats(&renderer, &scene);

    for (style, expected) in [
        (line_style(21.0, 2, -1.0, 0.0), "line width"),
        (line_style(21.0, 2, 2.0, 1.1), "distance fade"),
        (line_style(-1.0, 2, 2.0, 0.0), "connection distances"),
        (line_style(21.0, 0, 2.0, 0.0), "Max Neighbors"),
        (line_style(21.0, 33, 2.0, 0.0), "Max Neighbors"),
    ] {
        let mut invalid = scene.clone();
        invalid.render_style = style;
        let error = render_point_test_scene(&mut renderer, &invalid).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let recovered = render(&mut renderer, &scene);
    assert_eq!(recovered.data, baseline.data);
    assert_eq!(connections(&renderer, &scene), baseline_edges);
    assert_eq!(stats(&renderer, &scene), baseline_stats);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn sparse_hundred_thousand_points_stay_bounded_and_dense_failure_is_prompt() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    for (counts, expected_capacity) in [
        ([10, 10, 1], 100_u32),
        ([10, 10, 10], 1_000),
        ([100, 100, 1], 10_000),
        ([100, 100, 10], 100_000),
    ] {
        let mut scene = grid_scene(counts, [24.0, 24.0, 24.0]);
        scene.render_style = line_style(25.0, 6, 1.0, 0.0);
        let started = Instant::now();
        render(&mut renderer, &scene);
        let elapsed = started.elapsed();
        let observed = stats(&renderer, &scene);
        eprintln!(
            "sparse Point connections: capacity={expected_capacity} elapsed={elapsed:?} candidates={} max_per_point={} edges={} connection_bytes={} state_bytes={}",
            observed.candidate_tests,
            observed.max_point_candidates,
            observed.compact_edge_count,
            observed.connection_bytes,
            observed.allocated_bytes,
        );
        assert!(observed.max_point_candidates <= 128);
        assert!(observed.candidate_tests <= expected_capacity * 128);
        assert!(
            elapsed < Duration::from_secs(30),
            "sparse {expected_capacity}-Point proximity took {elapsed:?}"
        );
    }

    let mut dense = grid_scene([65, 65, 1], [0.0, 0.0, 0.0]);
    dense.render_style = line_style(80.0, 6, 1.0, 0.0);
    let started = Instant::now();
    let error = render_point_test_scene(&mut renderer, &dense).unwrap_err();
    let elapsed = started.elapsed();
    eprintln!("dense Point connection budget rejection: capacity=4225 elapsed={elapsed:?}");
    assert!(error.contains("Point proximity requires"), "{error}");
    assert!(error.contains("per-point"), "{error}");
    assert!(
        elapsed < Duration::from_secs(10),
        "dense work-budget rejection took {elapsed:?}"
    );
}
