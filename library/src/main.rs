use library::framing::get_frame_from_project;
use library::load_project;
use library::plugin::{ExportFormat, load_plugins};
use library::rendering::RenderContext;
use library::rendering::skia_renderer::SkiaRenderer;
use library::util::timing::{ScopedTimer, measure_debug, measure_info};
use log::{error, info};
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

  let json_str = measure_info(format!("Read project file {}", file_path), || {
    fs::read_to_string(file_path)
  })?;
  let proj = measure_info("Parse project JSON", || load_project(&json_str))?;

  if proj.compositions.is_empty() {
    error!("No compositions found in the project.");
    return Err("No compositions found".into());
  }

  let composition = proj.compositions.get(0).unwrap();

  let plugin_manager = load_plugins();
  let mut render_context = RenderContext::new(
    SkiaRenderer::new(
      composition.width as u32,
      composition.height as u32,
      composition.background_color.clone(),
    ),
    plugin_manager.clone(),
  );

  let total_frames = composition.duration.ceil().max(0.0) as u64;

  for frame_index in 0..total_frames {
    info!("Render frame {}:", frame_index);
    let _frame_scope = ScopedTimer::info(format!("Frame {} total", frame_index));

    measure_debug(
      format!("Frame {}: clear render target", frame_index),
      || render_context.clear(),
    )?;

    let frame_time = frame_index as f64 / composition.fps;
    let frame = measure_debug(
      format!("Frame {}: assemble frame graph", frame_index),
      || get_frame_from_project(&proj, 0, frame_time),
    );

    let img = measure_info(format!("Frame {}: renderer pass", frame_index), || {
      render_context.render_frame(frame)
    })?;

    let output_path = format!("./rendered/{}_{:03}.png", composition.name, frame_index);
    measure_info(format!("Frame {}: save image", frame_index), || {
      plugin_manager.export_image(ExportFormat::Png, &output_path, &img)
    })?;
  }
  info!("All frames rendered.");

  Ok(())
}
