//! Opt-in stage diagnostics on the production SceneRuntime. End-to-end release
//! measurements belong to the existing production_baseline benchmark instead.

use super::point_connection_gpu::{grid_scene, line_style, render, stats, transparent};
use super::*;
use std::time::Instant;

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn point_connection_gpu_stage_profile() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let driver = renderer
        .get_gpu_context()
        .expect("stage diagnostics require a real GPU")
        .driver_info()
        .unwrap();
    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_point_profiling_for_test(true)
        .unwrap();

    for (counts, neighbors) in [
        ([10, 10, 10], 6),
        ([100, 100, 1], 6),
        ([100, 100, 10], 1),
        ([100, 100, 10], 6),
        ([100, 100, 10], 32),
    ] {
        let mut scene = grid_scene(counts, [24.0; 3]);
        scene.render_style = line_style(25.0, neighbors, 1.0, 0.0);
        let reference = render(&mut renderer, &scene);
        let reference_stats = stats(&renderer, &scene);
        // Exclude allocation, shader compilation and first-use driver work.
        for _ in 0..2 {
            assert_eq!(render(&mut renderer, &scene).data, reference.data);
        }
        for sample in 0..5 {
            let started = Instant::now();
            let image = render(&mut renderer, &scene);
            let wall = started.elapsed();
            assert_eq!(
                image.data, reference.data,
                "warm draw must be deterministic"
            );
            assert_eq!(stats(&renderer, &scene), reference_stats);
            let profile = renderer
                .scene_runtime
                .as_ref()
                .unwrap()
                .point_profile_for_test()
                .expect("a successful profiled line draw must report its stages");
            assert_eq!(
                profile
                    .stages
                    .iter()
                    .map(|stage| stage.name)
                    .collect::<Vec<_>>(),
                [
                    "hash_clear_build",
                    "budget_gpu",
                    "search",
                    "mutual",
                    "scan_compact",
                    "draw"
                ]
            );
            let stages = profile
                .stages
                .iter()
                .map(|stage| (stage.name, stage.elapsed.as_nanos()))
                .collect::<std::collections::BTreeMap<_, _>>();
            println!(
                "point_stage_profile {}",
                serde_json::json!({
                    "gpu": driver.renderer,
                    "driver": driver.version,
                    "debug_assertions": cfg!(debug_assertions),
                    "target": [256, 144],
                    "counts": counts,
                    "max_neighbors": neighbors,
                    "sample": sample,
                    "wall_ns_including_test_queries_and_rgba_readback": wall.as_nanos(),
                    "budget_readback_wait_ns": profile.budget_readback_wait.as_nanos(),
                    "gpu_stages_ns": stages,
                    "candidate_tests": reference_stats.candidate_tests,
                    "max_point_candidates": reference_stats.max_point_candidates,
                    "compact_edge_count": reference_stats.compact_edge_count,
                })
            );
        }
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn point_connection_profiling_is_opt_in_and_clears_failed_reports() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let scene = grid_scene([3, 2, 1], [20.0; 3]);
    let ordinary = render(&mut renderer, &scene);
    assert!(renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .point_profile_for_test()
        .is_none());
    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_point_profiling_for_test(true)
        .unwrap();
    assert_eq!(render(&mut renderer, &scene).data, ordinary.data);
    assert!(renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .point_profile_for_test()
        .is_some());

    let mut invalid = scene.clone();
    invalid.render_style = line_style(21.0, 2, -1.0, 0.0);
    assert!(render_point_test_scene(&mut renderer, &invalid).is_err());
    assert!(renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .point_profile_for_test()
        .is_none());
    assert_eq!(render(&mut renderer, &scene).data, ordinary.data);

    let mut dense = grid_scene([65, 65, 1], [0.0; 3]);
    dense.render_style = line_style(80.0, 6, 1.0, 0.0);
    let error = render_point_test_scene(&mut renderer, &dense).unwrap_err();
    assert!(error.contains("candidate tests"), "{error}");
    assert!(renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .point_profile_for_test()
        .is_none());
    assert_eq!(render(&mut renderer, &scene).data, ordinary.data);
    assert!(renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .point_profile_for_test()
        .is_some());

    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_point_profiling_for_test(false)
        .unwrap();
    assert!(renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .point_profile_for_test()
        .is_none());
    assert_eq!(render(&mut renderer, &scene).data, ordinary.data);
    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_point_profiling_for_test(true)
        .unwrap();
    assert_eq!(render(&mut renderer, &scene).data, ordinary.data);
}
