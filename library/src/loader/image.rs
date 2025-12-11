use std::error::Error;

pub fn load_image(path: &str) -> Result<Image, Box<dyn Error>> {
    let img = image::open(path).map_err(|e| format!("画像ファイルを開けませんでした: {}", e))?;
    let rgba_image = img.to_rgba8();
    Ok(Image {
        width: rgba_image.width(),
        height: rgba_image.height(),
        data: rgba_image.into_raw(),
    })
}

#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self { width, height, data }
    }
}
