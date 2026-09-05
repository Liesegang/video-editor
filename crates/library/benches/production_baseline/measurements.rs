use std::fs;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use library::core::audio::authoring::AuthoringAudioMixer;
use library::core::cache::CacheManager;
use library::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use library::editor::{
    AuthoringPropertyOwner, RenderDestination, RenderService, TimelineEditorService,
    build_authoring_audio_e2e_fixture,
};
use library::model::authoring::{AuthoringProject, ProjectFileStore};
use library::model::frame::color::Color;
use library::model::project::property::PropertyValue;
use library::plugin::PluginManager;
use library::{RenderRequestId, RenderServer, SkiaRenderer};

use crate::BenchResult;
use crate::fixtures::FixtureSet;
use crate::report::{MetricDefinition, MetricResult, RunConfiguration, measure, unavailable};

const CONSECUTIVE_FRAMES: u32 = 30;
const AUDIO_WINDOW_FRAMES: usize = 4_800;

pub fn run(
    fixtures: &FixtureSet,
    configuration: RunConfiguration,
) -> BenchResult<Vec<MetricResult>> {
    let plugins = Arc::new(PluginManager::default());
    let mut metrics = vec![project_load(fixtures, configuration)?];
    metrics.push(render_plan_compile(
        "render_plan_compile_100_items",
        "timeline-items-100",
        &fixtures.items_100,
        100,
        configuration,
    )?);
    metrics.push(render_plan_compile(
        "render_plan_compile_1000_items",
        "timeline-items-1000",
        &fixtures.items_1_000,
        1_000,
        configuration,
    )?);
    metrics.push(render_plan_compile(
        "render_plan_compile_10000_items",
        "timeline-items-10000",
        &fixtures.items_10_000,
        10_000,
        configuration,
    )?);
    metrics.push(shared_module_compile(fixtures, configuration)?);
    metrics.push(first_frame(fixtures, &plugins, configuration)?);
    metrics.push(seek(fixtures, &plugins, configuration)?);
    metrics.push(consecutive_frames(fixtures, &plugins, configuration)?);
    metrics.push(edit_to_preview(fixtures, &plugins, configuration)?);
    metrics.extend(audio(fixtures, &plugins, configuration)?);
    metrics.push(single_frame_png_export(fixtures, &plugins, configuration)?);
    metrics.push(unavailable(
        "full_timeline_video_export",
        "export",
        "Encode and mux a complete Timeline video",
        "RenderServer authoring video worker -> ffmpeg_export",
        "not sampled by the portable baseline because the result depends on an explicitly configured external FFmpeg binary, codec, and destination filesystem; the PNG production export boundary is measured instead",
    ));
    Ok(metrics)
}

fn project_load(
    fixtures: &FixtureSet,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    measure(
        MetricDefinition {
            name: "project_load_1000_items",
            category: "project_load",
            description: "Read, deserialize, and validate a 1,000-item authoring Project",
            production_path: "ProjectFileStore::load -> ProjectDocument::from_json -> AuthoringProject::validate",
            fixture: "timeline-items-1000",
            operations_per_sample: 1,
        },
        configuration,
        || {
            let document = ProjectFileStore::load(&fixtures.load_project_path)?;
            black_box(document.project.items.len());
            Ok(())
        },
    )
}

fn render_plan_compile(
    name: &str,
    fixture: &str,
    project: &AuthoringProject,
    expected_items: usize,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    if project.items.len() != expected_items {
        return Err(format!("{fixture} has {} items", project.items.len()).into());
    }
    measure(
        MetricDefinition {
            name,
            category: "render_plan_compile",
            description: "Compile Timeline-owned placements into the hierarchical RenderPlan",
            production_path: "RenderPlanCompiler::compile",
            fixture,
            operations_per_sample: 1,
        },
        configuration,
        || {
            black_box(RenderPlanCompiler::compile(project)?);
            Ok(())
        },
    )
}

fn shared_module_compile(
    fixtures: &FixtureSet,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    let verification = RenderPlanCompiler::compile(&fixtures.shared_module_1_000)?;
    if verification.module_definitions.len() != 1 || verification.module_invocations.len() != 1_000
    {
        return Err(
            "shared Module fixture did not compile to one definition and 1,000 invocations".into(),
        );
    }
    measure(
        MetricDefinition {
            name: "render_plan_compile_shared_module_1000_instances",
            category: "render_plan_compile",
            description: "Compile 1,000 placements which share one compiled Module definition",
            production_path: "RenderPlanCompiler::compile",
            fixture: "shared-module-1000",
            operations_per_sample: 1,
        },
        configuration,
        || {
            black_box(RenderPlanCompiler::compile(&fixtures.shared_module_1_000)?);
            Ok(())
        },
    )
}

fn first_frame(
    fixtures: &FixtureSet,
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    measure(
        MetricDefinition {
            name: "first_frame_cpu_preview",
            category: "first_frame",
            description: "Cold compile, frame evaluation, CPU Skia initialization, and Preview raster",
            production_path: "RenderPlanCompiler -> evaluate_render_plan_frame -> RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
            fixture: "preview",
            operations_per_sample: 1,
        },
        configuration,
        || {
            let plan = RenderPlanCompiler::compile(&fixtures.preview_project)?;
            let frame = evaluate_render_plan_frame(
                &fixtures.preview_project,
                &plan,
                plugins.as_ref(),
                0,
                1.0,
                None,
            )?;
            let mut renderer = cpu_render_service(&fixtures.preview_project, Arc::clone(plugins))?;
            black_box(renderer.render_authoring_frame(
                &fixtures.preview_project,
                &frame,
                RenderDestination::Preview,
            )?);
            Ok(())
        },
    )
}

fn seek(
    fixtures: &FixtureSet,
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    let plan = RenderPlanCompiler::compile(&fixtures.preview_project)?;
    let mut renderer = cpu_render_service(&fixtures.preview_project, Arc::clone(plugins))?;
    let initial = evaluate_render_plan_frame(
        &fixtures.preview_project,
        &plan,
        plugins.as_ref(),
        0,
        1.0,
        None,
    )?;
    black_box(renderer.render_authoring_frame(
        &fixtures.preview_project,
        &initial,
        RenderDestination::Preview,
    )?);
    let mut iteration = 0_u64;
    measure(
        MetricDefinition {
            name: "seek_cpu_preview",
            category: "seek",
            description: "Evaluate and raster a discontinuous frame on a warm CPU Preview renderer",
            production_path: "evaluate_render_plan_frame -> RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
            fixture: "preview",
            operations_per_sample: 1,
        },
        configuration,
        || {
            let frame_number = if iteration.is_multiple_of(2) { 150 } else { 30 };
            iteration += 1;
            let frame = evaluate_render_plan_frame(
                &fixtures.preview_project,
                &plan,
                plugins.as_ref(),
                frame_number,
                1.0,
                None,
            )?;
            black_box(renderer.render_authoring_frame(
                &fixtures.preview_project,
                &frame,
                RenderDestination::Preview,
            )?);
            Ok(())
        },
    )
}

fn consecutive_frames(
    fixtures: &FixtureSet,
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    let plan = RenderPlanCompiler::compile(&fixtures.preview_project)?;
    let mut renderer = cpu_render_service(&fixtures.preview_project, Arc::clone(plugins))?;
    measure(
        MetricDefinition {
            name: "consecutive_cpu_preview_frames",
            category: "continuous_frames",
            description: "Evaluate and raster 30 consecutive frames on one warm CPU Preview renderer",
            production_path: "evaluate_render_plan_frame -> RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
            fixture: "preview",
            operations_per_sample: CONSECUTIVE_FRAMES,
        },
        configuration,
        || {
            for frame_number in 0..u64::from(CONSECUTIVE_FRAMES) {
                let frame = evaluate_render_plan_frame(
                    &fixtures.preview_project,
                    &plan,
                    plugins.as_ref(),
                    frame_number,
                    1.0,
                    None,
                )?;
                black_box(renderer.render_authoring_frame(
                    &fixtures.preview_project,
                    &frame,
                    RenderDestination::Preview,
                )?);
            }
            Ok(())
        },
    )
}

fn edit_to_preview(
    fixtures: &FixtureSet,
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    let editor = TimelineEditorService::new(fixtures.preview_project.clone())?;
    let mut renderer = cpu_render_service(&fixtures.preview_project, Arc::clone(plugins))?;
    let mut iteration = 0_u64;
    measure(
        MetricDefinition {
            name: "edit_to_cpu_preview",
            category: "edit_to_preview",
            description: "Commit one Inspector-style item property edit, snapshot, compile, evaluate, and raster Preview",
            production_path: "TimelineEditorService::set_authored_property_constant -> snapshot -> RenderPlanCompiler -> evaluate_render_plan_frame -> RenderService(Preview)",
            fixture: "preview",
            operations_per_sample: 1,
        },
        configuration,
        || {
            let opacity = if iteration.is_multiple_of(2) {
                0.75
            } else {
                0.5
            };
            iteration += 1;
            black_box(editor.set_authored_property_constant(
                AuthoringPropertyOwner::Item(fixtures.preview_item_id),
                "opacity".to_string(),
                PropertyValue::from(opacity),
            )?);
            let project = editor.snapshot()?;
            let plan = RenderPlanCompiler::compile(project.as_ref())?;
            let frame = evaluate_render_plan_frame(
                project.as_ref(),
                &plan,
                plugins.as_ref(),
                0,
                1.0,
                None,
            )?;
            black_box(renderer.render_authoring_frame(
                project.as_ref(),
                &frame,
                RenderDestination::Preview,
            )?);
            Ok(())
        },
    )
}

fn audio(
    fixtures: &FixtureSet,
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
) -> BenchResult<Vec<MetricResult>> {
    let fixture =
        build_authoring_audio_e2e_fixture(&fixtures.audio_media_directory, plugins.as_ref())?;
    let project = fixture.service.snapshot()?;
    let cold = measure(
        MetricDefinition {
            name: "audio_first_window",
            category: "audio",
            description: "Compile routes, decode, and mix the first 100 ms authoring audio window",
            production_path: "AuthoringAudioMixer::root -> render_window",
            fixture: "authoring-audio-e2e",
            operations_per_sample: 1,
        },
        configuration,
        || {
            let cache = CacheManager::new();
            let mut mixer = AuthoringAudioMixer::root(project.as_ref(), &cache)?;
            black_box(mixer.render_window(0, AUDIO_WINDOW_FRAMES)?);
            Ok(())
        },
    )?;

    let cache = CacheManager::new();
    let mut mixer = AuthoringAudioMixer::root(project.as_ref(), &cache)?;
    black_box(mixer.render_window(0, AUDIO_WINDOW_FRAMES)?);
    let warm = measure(
        MetricDefinition {
            name: "audio_cached_window",
            category: "audio",
            description: "Mix the same 100 ms authoring audio window from the production decode cache",
            production_path: "AuthoringAudioMixer::render_window",
            fixture: "authoring-audio-e2e",
            operations_per_sample: 1,
        },
        configuration,
        || {
            black_box(mixer.render_window(0, AUDIO_WINDOW_FRAMES)?);
            Ok(())
        },
    )?;
    Ok(vec![cold, warm])
}

fn single_frame_png_export(
    fixtures: &FixtureSet,
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
) -> BenchResult<MetricResult> {
    let project = Arc::new(fixtures.preview_project.clone());
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref())?);
    let timeline_id = project.root_timeline_id;
    let server = RenderServer::new(Arc::clone(plugins), Arc::new(CacheManager::new()));
    let directory = tempfile::tempdir()?;
    let mut iteration = 0_u64;
    measure(
        MetricDefinition {
            name: "single_frame_png_export",
            category: "export",
            description: "Evaluate, CPU-raster, color-terminate, and write one authoring frame as PNG",
            production_path: "RenderServer authoring PNG worker -> RenderService::render_authoring_export_frame -> png_export",
            fixture: "preview",
            operations_per_sample: 1,
        },
        configuration,
        || {
            let output_path = directory.path().join(format!("frame-{iteration}.png"));
            iteration += 1;
            if !server.send_authoring_png_export_request(
                RenderRequestId::new(iteration),
                Arc::clone(&project),
                Arc::clone(&plan),
                timeline_id,
                0,
                output_path.to_string_lossy().into_owned(),
            ) {
                return Err("production PNG export worker rejected the benchmark request".into());
            }
            let deadline = Instant::now() + Duration::from_secs(60);
            let completed = loop {
                match server.poll_authoring_export_result() {
                    Ok(completed) => break completed,
                    Err(TryRecvError::Empty) if Instant::now() < deadline => {
                        std::thread::yield_now();
                    }
                    Err(TryRecvError::Empty) => {
                        return Err("production PNG export benchmark timed out".into());
                    }
                    Err(TryRecvError::Disconnected) => {
                        return Err("production PNG export worker disconnected".into());
                    }
                }
            };
            completed.output?;
            if !completed.published {
                return Err("production PNG export completed without publishing".into());
            }
            black_box(fs::metadata(output_path)?.len());
            Ok(())
        },
    )
}

fn cpu_render_service(
    project: &AuthoringProject,
    plugins: Arc<PluginManager>,
) -> BenchResult<RenderService<SkiaRenderer>> {
    let timeline = project
        .timelines
        .get(&project.root_timeline_id)
        .ok_or("Project root Timeline is missing")?;
    let width = u32::try_from(timeline.width)?;
    let height = u32::try_from(timeline.height)?;
    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        width,
        height,
        Color::black(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )?;
    Ok(RenderService::new(renderer, plugins, cache))
}
