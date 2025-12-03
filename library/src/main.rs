use image::ColorType;
use library::framing::get_frame_from_project;
use library::load_project;
use library::rendering::RenderContext;
use library::rendering::skia_renderer::SkiaRenderer;
use std::env;
use std::error::Error;
use std::fs;
use std::time::Instant;
use log::{info, error};

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

  let json_str = fs::read_to_string(file_path)?;
  let proj = load_project(&json_str)?;
  
  if proj.compositions.is_empty() {
      error!("No compositions found in the project.");
      return Err("No compositions found".into());
  }

  let composition = proj.compositions.get(0).unwrap();

  let mut render_context = RenderContext::new(SkiaRenderer::new(
    composition.width as u32,
    composition.height as u32,
    composition.background_color.clone(),
  ));

  for frame_index in 0..composition.duration as u64 {
    info!("Render frame {}:", frame_index);
    let start_time = Instant::now();

    render_context.clear()?;

    let frame = get_frame_from_project(&proj, 0, frame_index as f64);
    let img = render_context.render_frame(frame)?;

    image::save_buffer(
      format!("./rendered/{}_{:03}.png", composition.name, frame_index),
      &img.data,
      img.width,
      img.height,
      ColorType::Rgba8,
    )?;
    info!(
      "Frame {} rendered in {} ms.",
      frame_index,
      start_time.elapsed().as_millis()
    );
  }
  info!("All frames rendered.");

  Ok(())
}
