use std::error::Error;

pub fn load_image(path: &String) -> Result<Image, Box<dyn Error>> {
    Ok(image::open(path)
        .map(|image| -> Image {
            let rgba_image = image.to_rgba8();
            Image {
                width: rgba_image.width(),
                height: rgba_image.height(),
                data: rgba_image.into_raw(),
            }
        })
        .map_err(|e| format!("画像ファイルを開けませんでした: {}", e))?)
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}
