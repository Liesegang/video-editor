pub mod animation;
pub mod audio;
pub mod cache;
pub mod error;
pub mod framing;
pub mod loader;
pub mod model;
pub mod plugin;
pub mod rendering;
pub mod service;
pub mod util;

pub use error::LibraryError;

pub use crate::loader::image::Image;
pub use crate::plugin::ExportSettings; // Added
// Re-export the services and models that the app will need.
pub use rendering::render_server::{RenderResult, RenderServer};
pub use rendering::skia_renderer::SkiaRenderer;
pub use service::{ExportService, ProjectModel, RenderService};

// use crate::plugin::load_plugins; // Removed
// use crate::rendering::effects::EffectRegistry; // Removed
use crate::framing::entity_converters::BuiltinEntityConverterPlugin;
use crate::plugin::PluginManager;
use log::info;
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::sync::Arc; // Added

// Function to create and initialize the PluginManager with built-in plugins
pub fn create_plugin_manager() -> Arc<PluginManager> {
    let manager = Arc::new(PluginManager::new());
    manager.register_effect(Arc::new(crate::plugin::effects::BlurEffectPlugin::new()));
    manager.register_effect(Arc::new(crate::plugin::effects::PixelSorterPlugin::new()));
    manager.register_effect(Arc::new(crate::plugin::effects::DilateEffectPlugin::new()));
    manager.register_effect(Arc::new(crate::plugin::effects::ErodeEffectPlugin::new()));
    manager.register_effect(Arc::new(
        crate::plugin::effects::DropShadowEffectPlugin::new(),
    ));
    manager.register_effect(Arc::new(
        crate::plugin::effects::MagnifierEffectPlugin::new(),
    ));
    manager.register_effect(Arc::new(crate::plugin::effects::TileEffectPlugin::new()));

    manager.register_load_plugin(Arc::new(crate::plugin::loaders::NativeImageLoader::new()));
    manager.register_load_plugin(Arc::new(crate::plugin::loaders::FfmpegVideoLoader::new()));
    manager.register_export_plugin(Arc::new(crate::plugin::exporters::PngExportPlugin::new()));
    manager.register_export_plugin(Arc::new(crate::plugin::exporters::FfmpegExportPlugin::new()));
    manager.register_property_plugin(Arc::new(
        crate::plugin::properties::ConstantPropertyPlugin::new(),
    ));
    manager.register_property_plugin(Arc::new(
        crate::plugin::properties::KeyframePropertyPlugin::new(),
    ));
    manager.register_property_plugin(Arc::new(
        crate::plugin::properties::ExpressionPropertyPlugin::new(),
    ));
    manager.register_entity_converter_plugin(Arc::new(BuiltinEntityConverterPlugin::new())); // Added
    manager
}

pub fn run(args: Vec<String>) -> Result<(), LibraryError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    if args.len() < 2 {
        return Err(LibraryError::InvalidArgument(
            "Please provide the path to a project JSON file.".to_string(),
        ));
    }

    if !fs::metadata("./rendered").is_ok() {
        info!("Creating ./rendered directory...");
        fs::create_dir("./rendered")?;
    }

    let file_path = &args[1];
    let project_model = ProjectModel::from_project_path(file_path, 0)?;

    let plugin_paths: Vec<_> = args
        .iter()
        .skip(2)
        .filter(|s| !s.starts_with("--"))
        .collect();
    let plugin_manager = create_plugin_manager(); // Changed
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

    // Removed effect_registry instantiation
    let composition = project_model.composition();
    let renderer = SkiaRenderer::new(
        composition.width as u32,
        composition.height as u32,
        composition.background_color.clone(),
        false,
        None,
    );

    let cache_manager = Arc::new(crate::cache::CacheManager::new());

    let _property_evaluators = plugin_manager.get_property_evaluators();

    let entity_converter_registry = plugin_manager.get_entity_converter_registry();

    let mut render_service = RenderService::new(
        renderer,
        plugin_manager.clone(),
        cache_manager.clone(),
        entity_converter_registry.clone(),
    );

    let mut export_settings = Arc::new(ExportSettings::from_project(
        project_model.project().as_ref(),
        project_model.composition(),
    )?);

    let total_frames = composition.duration.ceil().max(0.0) as u64;
    let final_frame_range = frame_range.unwrap_or(0..total_frames);
    let output_stem = format!("./rendered/{}", composition.name);

    // Audio Pre-rendering
    let mut audio_temp_path: Option<String> = None;
    if matches!(
        export_settings.export_format(),
        crate::plugin::ExportFormat::Video
    ) {
        let fps = composition.fps;
        let start_time = final_frame_range.start as f64 / fps;
        let duration_frames = (final_frame_range.end - final_frame_range.start).max(1);
        let duration = duration_frames as f64 / fps;

        // CLI currently defaults to 48000 as ProjectService is not initialized here
        let sample_rate = 48000;

        let start_sample = (start_time * sample_rate as f64).round() as u64;
        let frames = (duration * sample_rate as f64).round() as usize;

        let audio_data = crate::audio::mixer::mix_samples(
            &project_model.project().assets,
            project_model.composition(),
            &cache_manager,
            start_sample,
            frames,
            sample_rate,
            2,
        );

        if !audio_data.is_empty() {
            let audio_path = format!("{}_audio.raw", output_stem);
            if let Ok(mut file) = std::fs::File::create(&audio_path) {
                for sample in audio_data {
                    let _ = file.write_all(&sample.to_le_bytes());
                }

                if let Some(settings_mut) = Arc::get_mut(&mut export_settings) {
                    settings_mut.parameters.insert(
                        "audio_source".to_string(),
                        serde_json::Value::String(audio_path.clone()),
                    );
                    settings_mut.parameters.insert(
                        "audio_channels".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(2)),
                    );
                    settings_mut.parameters.insert(
                        "audio_sample_rate".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(sample_rate)),
                    );
                }
                audio_temp_path = Some(audio_path);
            }
        }
    }

    let mut export_service = ExportService::new(
        plugin_manager.clone(),
        "png_export".to_string(),
        export_settings,
        4,
    );

    export_service.render_range(
        &mut render_service,
        &project_model,
        final_frame_range,
        &output_stem,
    )?;
    info!("All frames rendered.");

    export_service.shutdown()?;

    // Cleanup audio temp file
    if let Some(path) = audio_temp_path {
        let _ = std::fs::remove_file(path);
    }

    Ok(())
}
