//! Opt-in measurements of the actual Preview renderer and terminal color stage.

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
use crate::report::{MetricDefinition, MetricResult, RunConfiguration, measure, unavailable};

const GPU_UNAVAILABLE_REASON: &str =
    "GPU measurement requires the opt-in --gpu-preview flag; the default workload selects CPU Skia";

#[derive(Clone, Copy)]
enum StyledVectorKind {
    Text,
    Shape,
}

#[derive(Clone, Copy)]
struct StyledVectorMetric {
    kind: StyledVectorKind,
    layers: usize,
    name: &'static str,
    fixture: &'static str,
}

const STYLED_VECTOR_METRICS: [StyledVectorMetric; 4] = [
    StyledVectorMetric {
        kind: StyledVectorKind::Text,
        layers: 1,
        name: "gpu_preview_4k_small_text_drop_shadow_1_layers",
        fixture: "styled-text-4k-1",
    },
    StyledVectorMetric {
        kind: StyledVectorKind::Text,
        layers: 16,
        name: "gpu_preview_4k_small_text_drop_shadow_16_layers",
        fixture: "styled-text-4k-16",
    },
    StyledVectorMetric {
        kind: StyledVectorKind::Shape,
        layers: 1,
        name: "gpu_preview_4k_small_shape_drop_shadow_1_layers",
        fixture: "styled-shape-4k-1",
    },
    StyledVectorMetric {
        kind: StyledVectorKind::Shape,
        layers: 16,
        name: "gpu_preview_4k_small_shape_drop_shadow_16_layers",
        fixture: "styled-shape-4k-16",
    },
];

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
        let frame = evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None)?;
        let (mut service, actual_driver) = warmed_gpu_service(&project, &frame, &plugins)?;
        validate_driver(&mut driver, actual_driver)?;
        metrics.push(measure(
            MetricDefinition {
                name: &format!("gpu_preview_raster_and_termination_{width}x{height}"),
                category: "preview",
                description: "Warm OpenGL Preview raster, GPU terminal color and RGBA8 readback; excludes UI upload",
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
    for definition in STYLED_VECTOR_METRICS {
        let projects = match definition.kind {
            StyledVectorKind::Text => &fixtures.styled_text_4k,
            StyledVectorKind::Shape => &fixtures.styled_shape_4k,
        };
        let project = &projects[usize::from(definition.layers == 16)];
        if project.items.len() != definition.layers {
            return Err(format!(
                "{} fixture has {} Timeline items, expected {}",
                definition.fixture,
                project.items.len(),
                definition.layers
            )
            .into());
        }
        let plan = RenderPlanCompiler::compile(project)?;
        let frame = evaluate_render_plan_frame(project, &plan, plugins.as_ref(), 0, 1.0, None)?;
        let (mut service, actual_driver) = warmed_gpu_service(project, &frame, &plugins)?;
        validate_driver(&mut driver, actual_driver)?;
        metrics.push(measure(
            MetricDefinition {
                name: definition.name,
                category: "preview",
                description: "Warm 4K OpenGL Preview of small descriptor-backed vector layers with Fill and Drop Shadow",
                production_path: "RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
                fixture: definition.fixture,
                operations_per_sample: 1,
            },
            configuration,
            || {
                black_box(service.render_authoring_frame(
                    project,
                    &frame,
                    RenderDestination::Preview,
                )?);
                Ok(())
            },
        )?);
    }
    Ok(GpuPreviewMeasurements {
        metrics,
        driver: driver.ok_or("no GPU workload ran")?,
    })
}

pub fn unavailable_metrics() -> Vec<MetricResult> {
    let mut metrics = vec![unavailable(
        "gpu_preview_frame",
        "preview",
        "Render one Preview frame on the active graphics device",
        "RenderService<SkiaRenderer> GPU backend",
        GPU_UNAVAILABLE_REASON,
    )];
    metrics.extend(STYLED_VECTOR_METRICS.map(|definition| {
        unavailable(
            definition.name,
            "preview",
            "Warm 4K OpenGL Preview of small descriptor-backed vector layers with Fill and Drop Shadow",
            "RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
            GPU_UNAVAILABLE_REASON,
        )
    }));
    metrics
}

fn warmed_gpu_service(
    project: &library::model::authoring::AuthoringProject,
    frame: &library::model::frame::frame::FrameInfo,
    plugins: &Arc<PluginManager>,
) -> BenchResult<(RenderService<SkiaRenderer>, GpuDriverInfo)> {
    let timeline = project
        .timelines
        .get(&project.root_timeline_id)
        .ok_or("GPU fixture root Timeline is missing")?;
    let width = u32::try_from(timeline.width)?;
    let height = u32::try_from(timeline.height)?;
    let cache = Arc::new(CacheManager::new());
    let mut renderer = SkiaRenderer::new(
        width,
        height,
        Color::black(),
        true,
        None,
        Some(Arc::clone(&cache)),
    )?;
    // The ordinary constructor permits a CPU fallback. Never report it as GPU data.
    let driver = renderer
        .get_gpu_context()
        .ok_or("--gpu-preview requires an active OpenGL renderer; CPU fallback rejected")?
        .driver_info()?;
    let mut service = RenderService::new(renderer, Arc::clone(plugins), cache);
    // Initialize the managed working surface and shaders outside warm-frame timing.
    black_box(service.render_authoring_frame(project, frame, RenderDestination::Preview)?);
    if !service.renderer.is_gpu_backed()? {
        return Err(
            "--gpu-preview requires a GPU-backed Project working surface; raster fallback rejected"
                .into(),
        );
    }
    if !service.renderer.last_terminal_was_gpu() {
        return Err("--gpu-preview expected the built-in Project GPU terminal stage; CPU termination rejected".into());
    }
    Ok((service, driver))
}

fn validate_driver(expected: &mut Option<GpuDriverInfo>, actual: GpuDriverInfo) -> BenchResult<()> {
    if expected
        .as_ref()
        .is_some_and(|previous| previous != &actual)
    {
        return Err("GPU device changed between Preview measurements".into());
    }
    *expected = Some(actual);
    Ok(())
}
