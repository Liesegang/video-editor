use image::ColorType;
use library::framing::get_frame_from_project;
use library::rendering::render_frame;
use library::load_project;
use library::rendering::skia_renderer::SkiaRenderer;
use std::env;
use std::error::Error;
use std::fs;
use std::time::Instant;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err("Please provide the path to a project JSON file.".into());
    }

    if !fs::metadata("./rendered").is_ok() {
        println!("Creating ./rendered directory...");
        fs::create_dir("./rendered")?;
    }

    let file_path = &args[1];

    let json_str = fs::read_to_string(file_path)?;
    let proj = load_project(&json_str)?;
    let composition = proj.compositions.get(0).unwrap();

    let mut renderer = SkiaRenderer::new(
        composition.width as u32,
        composition.height as u32,
        composition.background_color.clone(),
    );

    for frame_index in 0..composition.duration {
        println!("Render frame {}:", frame_index);
        let start_time = Instant::now();
        let frame = get_frame_from_project(&proj, 0, frame_index as f64);
        let img = render_frame(frame, &mut renderer)?;
        image::save_buffer(
            format!("./rendered/{}_{:03}.png", composition.name, frame_index),
            &img.data,
            img.width,
            img.height,
            ColorType::Rgba8,
        )?;
        println!("Frame {} rendered in {} ms.", frame_index, start_time.elapsed().as_millis());
    }
    println!("All frames rendered.");

    Ok(())
}
