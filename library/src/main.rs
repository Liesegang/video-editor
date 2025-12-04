use library::plugin::{load_plugins, ExportSettings};
use library::rendering::effects::EffectRegistry;
use library::rendering::skia_renderer::SkiaRenderer;
use library::service::{ExportService, ProjectModel, RenderService};
use log::info;
use std::env;
use std::error::Error;
use std::fs;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err("Please provide the path to a project JSON file.".into());
    }

    if !fs::metadata("./rendered").is_ok() {
        info!("Creating ./rendered directory...");
        fs::create_dir("./rendered")?;
    }

    let file_path = &args[1];
    let project_model = ProjectModel::from_project_path(file_path, 0)?;

    let plugin_manager = load_plugins();
    for plugin_path in &args[2..] {
        info!("Loading property plugin {}", plugin_path);
        plugin_manager.load_property_plugin_from_file(plugin_path)?;
    }

    let effect_registry = Arc::new(EffectRegistry::new_with_defaults());
    let composition = project_model.composition();
    let renderer = SkiaRenderer::new(
        composition.width as u32,
        composition.height as u32,
        composition.background_color.clone(),
    );
    let mut render_service = {
        let property_evaluators = Arc::new(plugin_manager.build_property_registry());
        RenderService::new(
            renderer,
            plugin_manager.clone(),
            property_evaluators,
            effect_registry,
        )
    };

    let export_settings = Arc::new(ExportSettings::from_project(
        project_model.project().as_ref(),
        project_model.composition(),
    ));

    let mut export_service = ExportService::new(plugin_manager, export_settings, 4);

    let total_frames = composition.duration.ceil().max(0.0) as u64;
    let output_stem = format!("./rendered/{}", composition.name);

    export_service.render_range(
        &mut render_service,
        &project_model,
        0..total_frames,
        &output_stem,
    )?;
    info!("All frames rendered.");

    export_service.shutdown()?;

    Ok(())
}