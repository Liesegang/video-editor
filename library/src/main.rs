use image::ColorType;
use std::error::Error;
use std::fs;
use std::env;
use library::render_frame_from_json;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err("Please provide the path to a JSON file.".into());
    }

    let file_path = &args[1];

    let json_str = fs::read_to_string(file_path)?;
    let img = render_frame_from_json(&json_str)?;

    println!(
        "Rendered image: width = {}, height = {}",
        img.width, img.height
    );
    println!("Image data size: {} bytes", img.data.len());

    image::save_buffer(
        "./result.png",
        &img.data,
        img.width,
        img.height,
        ColorType::Rgba8,
    )?;
    println!("Saved result.png");

    Ok(())
}
