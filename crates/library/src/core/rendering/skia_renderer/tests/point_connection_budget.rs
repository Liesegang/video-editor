//! Independent CPU oracle for the GPU spatial-hash work-budget counters.

use super::point_connection_gpu::{grid_scene, line_style, render, stats, transparent};
use super::point_invalidation_gpu::color_program;
use super::*;
use crate::model::frame::point::{PointGridParameters, PointRenderStyle};

const SATURATED_CANDIDATE_TESTS: u32 = 32_000_001;

fn hash_cell(cell: [i32; 3], mask: u32) -> u32 {
    let mut hash = (cell[0] as u32).wrapping_mul(0x8da6_b343)
        ^ (cell[1] as u32).wrapping_mul(0xd816_3841)
        ^ (cell[2] as u32).wrapping_mul(0xcb1a_b31f);
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    hash & mask
}

fn grid_positions(grid: &PointGridParameters) -> Vec<[f32; 3]> {
    let counts = grid.counts.map(|value| value as f32);
    let spacing = [
        grid.spacing.x.0 as f32,
        grid.spacing.y.0 as f32,
        grid.spacing.z.0 as f32,
    ];
    let center = [
        grid.center.x.0 as f32,
        grid.center.y.0 as f32,
        grid.center.z.0 as f32,
    ];
    let mut points = Vec::with_capacity(grid.capacity() as usize);
    for z in 0..grid.counts[2] {
        for y in 0..grid.counts[1] {
            for x in 0..grid.counts[0] {
                let coordinate = [x as f32, y as f32, z as f32];
                points.push(std::array::from_fn(|axis| {
                    center[axis] + (coordinate[axis] - (counts[axis] - 1.0) * 0.5) * spacing[axis]
                }));
            }
        }
    }
    points
}

fn line_maximum(scene: &PointSceneFrame) -> f32 {
    let PointRenderStyle::Lines { connections, .. } = &scene.render_style else {
        panic!("Point budget oracle requires Line render style")
    };
    connections.max_distance.0
}

/// Mirrors only the documented hash-work accounting, independently of the
/// topology search and compact-edge implementation.
fn reference_budget(
    capacity: u32,
    valid_positions: &[[f32; 3]],
    maximum_distance: f32,
) -> (u32, u32) {
    assert!(maximum_distance > 0.0);
    let bucket_count = capacity.checked_mul(2).unwrap().next_power_of_two();
    let mask = bucket_count - 1;
    let cells = valid_positions
        .iter()
        .map(|position| position.map(|component| (component / maximum_distance).floor() as i32))
        .collect::<Vec<_>>();
    let mut bucket_sizes = vec![0_u32; bucket_count as usize];
    for cell in &cells {
        bucket_sizes[hash_cell(*cell, mask) as usize] += 1;
    }

    let mut total = 0_u32;
    let mut maximum = 0_u32;
    for cell in cells {
        let mut unique = Vec::with_capacity(27);
        let mut work = 0_u32;
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    let bucket = hash_cell([cell[0] + x, cell[1] + y, cell[2] + z], mask);
                    if !unique.contains(&bucket) {
                        unique.push(bucket);
                        work += bucket_sizes[bucket as usize];
                    }
                }
            }
        }
        maximum = maximum.max(work);
        total = total.saturating_add(work).min(SATURATED_CANDIDATE_TESTS);
    }
    (total, maximum)
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn hash_budget_matches_cpu_oracle_across_dispatch_and_scan_boundaries() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    for capacity in [1_u32, 63, 64, 65, 255, 257] {
        let mut scene = grid_scene([capacity, 1, 1], [24.0, 0.0, 0.0]);
        scene.render_style = line_style(25.0, 6, 1.0, 0.0);
        render(&mut renderer, &scene);
        let PointSceneSource::Grid(grid) = &scene.source else {
            panic!("Grid fixture")
        };
        let expected = reference_budget(
            scene.source.capacity(),
            &grid_positions(grid),
            line_maximum(&scene),
        );
        let observed = stats(&renderer, &scene);
        assert_eq!(
            (observed.candidate_tests, observed.max_point_candidates),
            expected,
            "capacity {capacity}"
        );
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn dead_particle_slots_do_not_contribute_to_hash_budget() {
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
    let positions = fields
        .iter()
        .map(|point| {
            point
                .position
                .or(point.source_position)
                .expect("active Particle position")
        })
        .collect::<Vec<_>>();
    let expected = reference_budget(scene.source.capacity(), &positions, line_maximum(&scene));
    let observed = stats(&renderer, &scene);
    assert_eq!(
        (observed.candidate_tests, observed.max_point_candidates),
        expected
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn coordinate_and_per_point_budget_failures_leave_runtime_recoverable() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let safe = grid_scene([8, 1, 1], [24.0, 0.0, 0.0]);
    let safe_pixels = render(&mut renderer, &safe);

    let mut coordinate_error = grid_scene([1, 1, 1], [0.0; 3]);
    let PointSceneSource::Grid(grid) = &mut coordinate_error.source else {
        panic!("Grid fixture")
    };
    grid.center = particle_vec3(1_000_000.0, 0.0, 0.0);
    coordinate_error.render_style = line_style(0.000_001, 1, 1.0, 0.0);
    let error = render_point_test_scene(&mut renderer, &coordinate_error).unwrap_err();
    assert!(error.contains("cell coordinates exceed"), "{error}");
    assert_eq!(render(&mut renderer, &safe).data, safe_pixels.data);

    let mut dense = grid_scene([65, 65, 1], [0.0; 3]);
    dense.render_style = line_style(80.0, 6, 1.0, 0.0);
    let expected = reference_budget(
        dense.source.capacity(),
        &match &dense.source {
            PointSceneSource::Grid(grid) => grid_positions(grid),
            PointSceneSource::Particle { .. } => panic!("expected Grid fixture"),
        },
        line_maximum(&dense),
    );
    let error = render_point_test_scene(&mut renderer, &dense).unwrap_err();
    assert!(error.contains("per-point"), "{error}");
    assert_eq!(expected.1, dense.source.capacity());
    let observed = stats(&renderer, &dense);
    assert_eq!(
        (observed.candidate_tests, observed.max_point_candidates),
        expected
    );
    assert_eq!(render(&mut renderer, &safe).data, safe_pixels.data);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn globally_saturated_budget_is_exact_and_recovers_without_per_point_overflow() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut dense = grid_scene([1_024, 32, 1], [0.0, 200.0, 0.0]);
    dense.render_style = line_style(80.0, 6, 1.0, 0.0);
    let points = match &dense.source {
        PointSceneSource::Grid(grid) => grid_positions(grid),
        PointSceneSource::Particle { .. } => panic!("expected Grid fixture"),
    };
    let expected = reference_budget(dense.source.capacity(), &points, line_maximum(&dense));
    assert_eq!(expected.0, SATURATED_CANDIDATE_TESTS);
    assert!(
        expected.1 <= 4_096,
        "fixture must isolate global saturation"
    );

    let error = render_point_test_scene(&mut renderer, &dense).unwrap_err();
    assert!(
        error.contains("requires 32000001 candidate tests"),
        "{error}"
    );
    assert!(
        error.contains(&format!("per-point {} / maximum 4096", expected.1)),
        "{error}"
    );
    let observed = stats(&renderer, &dense);
    assert_eq!(
        (observed.candidate_tests, observed.max_point_candidates),
        expected
    );

    let safe = grid_scene([8, 1, 1], [24.0, 0.0, 0.0]);
    render(&mut renderer, &safe);
    let PointSceneSource::Grid(grid) = &safe.source else {
        panic!("Grid fixture")
    };
    let expected_safe = reference_budget(
        safe.source.capacity(),
        &grid_positions(grid),
        line_maximum(&safe),
    );
    let recovered = stats(&renderer, &safe);
    assert_eq!(
        (recovered.candidate_tests, recovered.max_point_candidates),
        expected_safe
    );
}
