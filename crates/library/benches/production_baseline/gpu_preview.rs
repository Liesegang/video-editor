//! Opt-in measurements of the actual Preview renderer, including CPU termination.

use std::hint::black_box;
use std::sync::Arc;

use library::SkiaRenderer;
use library::core::cache::CacheManager;
use library::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use library::editor::{RenderDestination, RenderService};
use library::model::frame::color::Color;
use library::plugin::PluginManager;
use library::rendering::renderer::Renderer;
use library::rendering::skia_utils::GpuDriverInfo;

use crate::BenchResult;
use crate::fixtures::FixtureSet;
use crate::report::{MetricDefinition, MetricResult, RunConfiguration, measure};

pub struct GpuPreviewMeasurements {
    pub metrics: Vec<MetricResult>,
    pub driver: GpuDriverInfo,
}

pub fn run(
    fixtures: &FixtureSet,
    configuration: RunConfiguration,
) -> BenchResult<GpuPreviewMeasurements> {
    let plugins = Arc::new(PluginManager::default());
    let mut metrics = Vec::new();
    let mut driver = None;
    for (width, height) in [(320_u32, 180_u32), (1920, 1080)] {
        let mut project = fixtures.preview_project.clone();
        let timeline = project
            .timelines
            .get_mut(&project.root_timeline_id)
            .ok_or("GPU fixture root Timeline is missing")?;
        timeline.width = u64::from(width);
        timeline.height = u64::from(height);
        let plan = RenderPlanCompiler::compile(&project)?;
        let fixture_name = format!("preview-4-solids-{width}x{height}");
        let mut frame_number = 0;
        metrics.push(measure(
            MetricDefinition {
                name: &format!("frame_evaluation_{width}x{height}"),
                category: "frame_evaluation",
                description: "Evaluate four Timeline solids without rasterization",
                production_path: "evaluate_render_plan_frame",
                fixture: &fixture_name,
                operations_per_sample: 30,
            },
            configuration,
            || {
                for _ in 0..30 {
                    frame_number += 1;
                    black_box(evaluate_render_plan_frame(
                        &project,
                        &plan,
                        plugins.as_ref(),
                        frame_number,
                        1.0,
                        None,
                    )?);
                }
                Ok(())
            },
        )?);
        let cache = Arc::new(CacheManager::new());
        let mut renderer = SkiaRenderer::new(
            width,
            height,
            Color::black(),
            true,
            None,
            Some(Arc::clone(&cache)),
        )?;
        // The ordinary constructor permits a CPU fallback. Never report that as GPU data.
        let actual_driver = renderer
            .get_gpu_context()
            .ok_or("--gpu-preview requires an active OpenGL renderer; CPU fallback rejected")?
            .driver_info()?;
        if driver
            .as_ref()
            .is_some_and(|previous| previous != &actual_driver)
        {
            return Err("GPU device changed between measurement sizes".into());
        }
        driver = Some(actual_driver);
        let mut service = RenderService::new(renderer, Arc::clone(&plugins), cache);
        let frame = evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None)?;
        // Initialize the managed working surface and shaders outside warm-frame timing.
        black_box(service.render_authoring_frame(&project, &frame, RenderDestination::Preview)?);
        if !service.renderer.is_gpu_backed()? {
            return Err("--gpu-preview requires a GPU-backed Project working surface; raster fallback rejected".into());
        }
        metrics.push(measure(
            MetricDefinition {
                name: &format!("gpu_preview_raster_and_termination_{width}x{height}"),
                category: "preview",
                description: "Warm OpenGL Preview raster, working-pixel readback and CPU display termination; excludes UI upload",
                production_path: "RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
                fixture: &fixture_name,
                operations_per_sample: 3,
            },
            configuration,
            || {
                for _ in 0..3 {
                    black_box(service.render_authoring_frame(
                        &project, &frame, RenderDestination::Preview,
                    )?);
                }
                Ok(())
            },
        )?);
    }
    Ok(GpuPreviewMeasurements {
        metrics,
        driver: driver.ok_or("no GPU workload ran")?,
    })
}
