use library::plugin::ExportFormat;
use library::service::ProjectService;
use log::info;
use std::env;
use std::error::Error;
use std::fs;

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

    let mut render_service = ProjectService::from_project_path(file_path, 0, 4)?;

    for plugin_path in &args[2..] {
        info!("Loading property plugin {}", plugin_path);
        render_service.load_property_plugin(plugin_path)?;
    }
    let composition = render_service.composition().clone();
    let total_frames = composition.duration.ceil().max(0.0) as u64;
    let output_stem = format!("./rendered/{}", composition.name);

    render_service.render_range(0..total_frames, &output_stem, ExportFormat::Png)?;
    info!("All frames rendered.");

    render_service.shutdown()?;

    Ok(())
}
