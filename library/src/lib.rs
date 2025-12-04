pub mod animation;
pub mod cache;
pub mod framing;
pub mod loader;
pub mod model;
pub mod plugin;
pub mod rendering;
pub mod service;
pub mod util;

pub use crate::loader::image::Image;
// Re-export the services and models that the app will need.
pub use service::{ExportService, ProjectModel, RenderService};
pub use rendering::skia_renderer::SkiaRenderer;

use crate::plugin::{load_plugins, ExportSettings};
use crate::rendering::effects::EffectRegistry;
use log::info;
use std::error::Error;
use std::fs;
use std::ops::Range;
use std::sync::Arc;

pub fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    env_logger::init();

    if args.len() < 2 {
        return Err("Please provide the path to a project JSON file.".into());
    }

    if !fs::metadata("./rendered").is_ok() {
        info!("Creating ./rendered directory...");
        fs::create_dir("./rendered")?;
    }

    let file_path = &args[1];
    let project_model = ProjectModel::from_project_path(file_path, 0)?;

    let plugin_paths: Vec<_> = args.iter().skip(2).filter(|s| !s.starts_with("--")).collect();
    let plugin_manager = load_plugins();
    for plugin_path in plugin_paths {
        info!("Loading property plugin {}", plugin_path);
        plugin_manager.load_property_plugin_from_file(plugin_path)?;
    }

    let mut frame_range: Option<Range<u64>> = None;
    if let Some(frames_arg_pos) = args.iter().position(|s| s == "--frames") {
        if let Some(range_str) = args.get(frames_arg_pos + 1) {
            if let Some(separator_pos) = range_str.find('-') {
                let start_str = &range_str[..separator_pos];
                let end_str = &range_str[separator_pos + 1..];
                if let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
                    frame_range = Some(start..end + 1);
                }
            } else if let Ok(single_frame) = range_str.parse::<u64>() {
                frame_range = Some(single_frame..single_frame + 1);
            }
        }
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
    let final_frame_range = frame_range.unwrap_or(0..total_frames);
    let output_stem = format!("./rendered/{}", composition.name);

    export_service.render_range(
        &mut render_service,
        &project_model,
        final_frame_range,
        &output_stem,
    )?;
    info!("All frames rendered.");

    export_service.shutdown()?;

    Ok(())
}
