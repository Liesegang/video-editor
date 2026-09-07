//! Independent CPU topology oracle for the production GPU Point output.

use super::point_connection_gpu::{
    connections, edge_pairs, grid_scene, line_style, render, scaled_position_program, stats,
    transparent,
};
use super::point_invalidation_gpu::color_program;
use super::*;
use crate::model::frame::point::{
    PointConnectionParameters, PointGridParameters, PointRenderStyle,
};
use crate::rendering::scene_runtime::PointConnectionReadback;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug)]
struct ReferencePoint {
    serial: u32,
    position: [f32; 3],
}

fn reference_connections(
    points: &[ReferencePoint],
    parameters: &PointConnectionParameters,
) -> Vec<PointConnectionReadback> {
    let minimum2 = parameters.min_distance.0 * parameters.min_distance.0;
    let maximum2 = parameters.max_distance.0 * parameters.max_distance.0;
    let mut nearest = BTreeMap::<u32, Vec<(u32, f32)>>::new();
    for source in points {
        let mut candidates = points
            .iter()
            .filter(|target| target.serial != source.serial)
            .filter_map(|target| {
                let delta = std::array::from_fn::<_, 3, _>(|axis| {
                    target.position[axis] - source.position[axis]
                });
                let distance2 = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
                (distance2 >= minimum2 && distance2 <= maximum2)
                    .then_some((target.serial, distance2))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        candidates.truncate(parameters.max_neighbors as usize);
        nearest.insert(source.serial, candidates);
    }

    let selected = nearest
        .iter()
        .map(|(source, targets)| {
            (
                *source,
                targets
                    .iter()
                    .map(|(target, _)| *target)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for (source, targets) in &nearest {
        for (target, distance2) in targets {
            if source < target && selected.get(target).is_some_and(|set| set.contains(source)) {
                result.push(PointConnectionReadback {
                    source_serial: *source,
                    target_serial: *target,
                    distance: distance2.sqrt(),
                });
            }
        }
    }
    result.sort_by_key(|edge| (edge.source_serial, edge.target_serial));
    result
}

fn grid_points(parameters: &PointGridParameters, scale_x: f32) -> Vec<ReferencePoint> {
    let mut points = Vec::with_capacity(parameters.capacity() as usize);
    for z in 0..parameters.counts[2] {
        for y in 0..parameters.counts[1] {
            for x in 0..parameters.counts[0] {
                let coordinate = [x, y, z];
                let index = coordinate.map(|value| value as f32);
                let counts = parameters.counts.map(|value| value as f32);
                let spacing = [
                    parameters.spacing.x.0 as f32,
                    parameters.spacing.y.0 as f32,
                    parameters.spacing.z.0 as f32,
                ];
                let center = [
                    parameters.center.x.0 as f32,
                    parameters.center.y.0 as f32,
                    parameters.center.z.0 as f32,
                ];
                let mut position = std::array::from_fn(|axis| {
                    center[axis] + (index[axis] - (counts[axis] - 1.0) * 0.5) * spacing[axis]
                });
                position[0] *= scale_x;
                points.push(ReferencePoint {
                    serial: parameters.point_serial(coordinate).unwrap(),
                    position,
                });
            }
        }
    }
    points
}

fn line_parameters(scene: &PointSceneFrame) -> &PointConnectionParameters {
    let PointRenderStyle::Lines { connections, .. } = &scene.render_style else {
        panic!("Point topology oracle requires Line render style")
    };
    connections
}

fn assert_matches_reference(
    actual: &[PointConnectionReadback],
    expected: &[PointConnectionReadback],
) {
    assert_eq!(edge_pairs(actual), edge_pairs(expected));
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(actual.source_serial < actual.target_serial);
        assert!(
            (actual.distance - expected.distance).abs() <= 1e-4,
            "edge {}-{} distance {} != CPU reference {}",
            actual.source_serial,
            actual.target_serial,
            actual.distance,
            expected.distance,
        );
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_topology_matches_independent_reference_for_k_extremes_ties_and_inclusive_bounds() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    for (counts, spacing, minimum, maximum, neighbors) in [
        ([5, 1, 1], [10.0, 10.0, 10.0], 0.0, 10.1, 1),
        ([3, 2, 1], [20.0, 20.0, 20.0], 0.0, 21.0, 2),
        ([3, 3, 1], [10.0, 10.0, 10.0], 10.0, 10.0, 6),
        ([3, 3, 2], [8.0, 9.0, 10.0], 0.0, 100.0, 32),
    ] {
        let mut scene = grid_scene(counts, spacing);
        scene.render_style = PointRenderStyle::Lines {
            connections: PointConnectionParameters {
                min_distance: minimum.into(),
                max_distance: maximum.into(),
                max_neighbors: neighbors,
            },
            width: 1.0.into(),
            fade: 0.0.into(),
        };
        render(&mut renderer, &scene);
        let PointSceneSource::Grid(grid) = &scene.source else {
            panic!("Grid fixture")
        };
        let expected = reference_connections(&grid_points(grid, 1.0), line_parameters(&scene));
        let actual = connections(&renderer, &scene);
        assert_matches_reference(&actual, &expected);
        assert!(actual
            .iter()
            .all(|edge| edge.distance >= minimum && edge.distance <= maximum));
    }

    let mut excluded = grid_scene([4, 1, 1], [10.0, 10.0, 10.0]);
    excluded.render_style = line_style(9.999, 32, 1.0, 0.0);
    render(&mut renderer, &excluded);
    assert!(connections(&renderer, &excluded).is_empty());
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_topology_uses_post_set_position_3d_geometry_and_stays_warm() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = grid_scene([2, 2, 2], [10.0, 10.0, 10.0]);
    scene.render_style = line_style(11.0, 32, 2.0, 0.0);
    scene.point_program = Some(scaled_position_program(2.0));
    let first_pixels = render(&mut renderer, &scene);
    let first = connections(&renderer, &scene);
    let first_stats = stats(&renderer, &scene);
    let PointSceneSource::Grid(grid) = &scene.source else {
        panic!("Grid fixture")
    };
    let expected = reference_connections(&grid_points(grid, 2.0), line_parameters(&scene));
    assert_matches_reference(&first, &expected);
    assert_eq!(first.len(), 8, "x-scaled cube retains only y/z axis edges");

    let second_pixels = render(&mut renderer, &scene);
    let second = connections(&renderer, &scene);
    assert_eq!(second_pixels.data, first_pixels.data);
    assert_eq!(second, first);
    let second_stats = stats(&renderer, &scene);
    assert_eq!(
        second_stats.connection_generation,
        first_stats.connection_generation
    );
    assert_eq!(second_stats.candidate_tests, first_stats.candidate_tests);
    assert_eq!(
        second_stats.compact_edge_count,
        first_stats.compact_edge_count
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn particle_survivor_holes_match_the_same_cpu_mutual_oracle_after_warm_render() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = particle_scene(600);
    scene.color = Color::white();
    scene.render_style = line_style(40.0, 6, 1.0, 0.0);
    scene.point_program = Some(color_program(Color::white()));
    render(&mut renderer, &scene);
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert!(!fields.is_empty());
    assert!(fields.len() < scene.source.capacity() as usize);
    assert!(fields.iter().all(|point| point.serial >= 120));
    assert!(fields
        .iter()
        .any(|point| point.serial >= fields.len() as u32));
    let points = fields
        .iter()
        .map(|point| ReferencePoint {
            serial: point.serial,
            position: point.source_position.expect("Particle source position"),
        })
        .collect::<Vec<_>>();
    let expected = reference_connections(&points, line_parameters(&scene));
    let first = connections(&renderer, &scene);
    assert_matches_reference(&first, &expected);
    let first_stats = stats(&renderer, &scene);

    render(&mut renderer, &scene);
    assert_eq!(connections(&renderer, &scene), first);
    let second_stats = stats(&renderer, &scene);
    assert_eq!(
        second_stats.simulation_generation,
        first_stats.simulation_generation
    );
    assert_eq!(second_stats.simulated_steps, first_stats.simulated_steps);
    assert_eq!(
        second_stats.connection_generation,
        first_stats.connection_generation
    );
}
