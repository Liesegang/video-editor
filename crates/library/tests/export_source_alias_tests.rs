use library::core::cache::CacheManager;
use library::editor::{ExportService, ProjectModel, RenderService};
use library::model::frame::color::Color;
use library::model::project::asset::{Asset, AssetKind};
use library::model::project::{
    ColorConfigIdentity, ColorManagementConfig, Composition, ExportColorConfig, PreviewColorConfig,
    PreviewSurfaceEncoding, Project,
};
use library::plugin::{ExportFrame, ExportPlugin, ExportSettings, Plugin, PluginManager};
use library::{LibraryError, SkiaRenderer};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .context("temporary test path must be UTF-8")
        .map(str::to_string)
}

fn test_project(name: &str) -> Result<Project> {
    let mut project = Project::new(name);
    let (composition, track) = Composition::new("main", 1, 1, 24.0, 1.0);
    project.add_track(track)?;
    project.add_composition(composition)?;
    Ok(project)
}

fn model_with_source(path: String, kind: AssetKind) -> Result<ProjectModel> {
    let mut project = test_project("export source alias")?;
    project
        .assets
        .push(Asset::new("protected source", &path, kind));
    Ok(ProjectModel::new(Arc::new(project), 0)?)
}

fn video_settings(model: &ProjectModel, container: &str) -> Result<ExportSettings> {
    let mut settings = ExportSettings::from_project(model.project().as_ref(), model.composition())?;
    settings.container = container.to_string();
    settings.codec = "libx264".to_string();
    settings.pixel_format = "yuv420p".to_string();
    Ok(settings)
}

fn expect_alias_rejection(
    model: &ProjectModel,
    settings: &ExportSettings,
    output_stem: &Path,
    range: std::ops::Range<u64>,
) -> Result<()> {
    let Err(error) = ExportService::verify_plan(model, settings, range, &path_string(output_stem)?)
    else {
        bail!("source alias was accepted before an export plan existed");
    };
    ensure!(
        error.to_string().contains("aliases Project asset"),
        "{error}"
    );
    ensure!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
    Ok(())
}

#[test]
fn video_output_cannot_be_the_source_file() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"source bytes")?;
    let model = model_with_source(path_string(&source)?, AssetKind::Video)?;
    expect_alias_rejection(&model, &video_settings(&model, "mp4")?, &source, 0..1)?;
    ensure!(std::fs::read(source)? == b"source bytes");
    Ok(())
}

#[test]
fn relative_source_and_absolute_output_are_the_same_identity() -> Result<()> {
    let current = std::env::current_dir()?.canonicalize()?;
    let directory = tempfile::tempdir_in(&current)?;
    let source = directory.path().join("relative-source.mp4");
    std::fs::write(&source, b"source bytes")?;
    let absolute = source.canonicalize()?;
    let relative = absolute.strip_prefix(&current)?;
    let model = model_with_source(path_string(relative)?, AssetKind::Video)?;
    expect_alias_rejection(&model, &video_settings(&model, "mp4")?, &absolute, 0..1)?;
    Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn existing_hardlink_output_is_rejected() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("source.mp4");
    let output = directory.path().join("hardlink.mp4");
    std::fs::write(&source, b"source bytes")?;
    std::fs::hard_link(&source, &output)?;
    let model = model_with_source(path_string(&source)?, AssetKind::Video)?;
    expect_alias_rejection(&model, &video_settings(&model, "mp4")?, &output, 0..1)?;
    Ok(())
}

#[test]
fn numbered_png_destination_cannot_collide_with_a_source() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let stem = directory.path().join("sequence");
    let source = directory.path().join("sequence_012.png");
    std::fs::write(&source, b"source bytes")?;
    let model = model_with_source(path_string(&source)?, AssetKind::Image)?;
    let settings = ExportSettings::from_project(model.project().as_ref(), model.composition())?;
    expect_alias_rejection(&model, &settings, &stem, 12..13)?;
    Ok(())
}

struct CountingExporter(Arc<AtomicUsize>);

impl Plugin for CountingExporter {
    fn id(&self) -> &'static str {
        "source_alias_counting_export"
    }

    fn name(&self) -> String {
        "Source Alias Counting Export".to_string()
    }

    fn category(&self) -> String {
        "Export".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for CountingExporter {
    fn export_frame(
        &self,
        _path: &str,
        _frame: &ExportFrame,
        _settings: &ExportSettings,
    ) -> std::result::Result<(), LibraryError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn counting_plugins(callbacks: &Arc<AtomicUsize>) -> Arc<PluginManager> {
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(CountingExporter(Arc::clone(callbacks))));
    plugins
}

#[test]
fn rejection_has_no_exporter_callback_and_safe_output_exports() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let unsafe_stem = directory.path().join("unsafe");
    let source = directory.path().join("unsafe_000.png");
    let safe_stem = directory.path().join("safe");
    std::fs::write(&source, b"source bytes")?;
    let model = model_with_source(path_string(&source)?, AssetKind::Image)?;
    let settings = ExportSettings::from_project(model.project().as_ref(), model.composition())?;
    let callbacks = Arc::new(AtomicUsize::new(0));
    let plugins = counting_plugins(&callbacks);

    expect_alias_rejection(&model, &settings, &unsafe_stem, 0..1)?;
    ensure!(callbacks.load(Ordering::SeqCst) == 0);

    let plan = ExportService::verify_plan(&model, &settings, 0..1, &path_string(&safe_stem)?)?;
    let mut service = ExportService::new(
        Arc::clone(&plugins),
        "source_alias_counting_export".to_string(),
        Arc::new(settings),
        plan,
        1,
    )?;
    let renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None)?;
    let mut render_service = RenderService::new(renderer, plugins, Arc::new(CacheManager::new()));
    service.render_range(&mut render_service, &model, 0..1)?;
    service.shutdown()?;
    ensure!(callbacks.load(Ordering::SeqCst) == 1);
    Ok(())
}

#[test]
fn referenced_project_asset_ocio_config_is_a_protected_source() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let config_path = directory.path().join("show-v1.ocio");
    let bytes = b"ocio_profile_version: 2\n";
    std::fs::write(&config_path, bytes)?;
    let mut config_asset = Asset::new("show config", &path_string(&config_path)?, AssetKind::Other);
    let checksum = config_asset.verify_imported_content(bytes);
    let identity = ColorConfigIdentity::ProjectAsset {
        asset_id: config_asset.id,
        sha256: checksum,
        ocio_version: "2.5.2".to_string(),
    };
    let mut project = test_project("external config alias")?;
    project.assets.push(config_asset);
    project
        .set_color_management(
            ColorManagementConfig::new(
                identity,
                "fixture-linear",
                PreviewColorConfig::named_view(
                    "fixture-display",
                    "fixture-view",
                    "fixture-srgb",
                    PreviewSurfaceEncoding::Srgb,
                ),
                ExportColorConfig::new("fixture-srgb"),
            )
            .with_srgb_surface_space("fixture-srgb"),
        )
        .map_err(|issues| anyhow::anyhow!("invalid external config test Project: {issues:?}"))?;
    let model = ProjectModel::new(Arc::new(project), 0)?;
    let settings = video_settings(&model, "ocio")?;
    expect_alias_rejection(&model, &settings, &config_path, 0..1)?;
    Ok(())
}

#[test]
fn generated_asset_without_a_file_path_does_not_block_export() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let model = model_with_source(String::new(), AssetKind::Other)?;
    let settings = ExportSettings::from_project(model.project().as_ref(), model.composition())?;
    ExportService::verify_plan(
        &model,
        &settings,
        0..1,
        &path_string(&directory.path().join("safe"))?,
    )?;
    Ok(())
}
use anyhow::{Context, Result, bail, ensure};
