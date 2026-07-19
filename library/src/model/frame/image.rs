//! Image data structure for decoded frames.

/// Represents straight (unpremultiplied) RGBA8 pixels in row-major order.
///
/// RGB channels retain their authored values for partially transparent pixels.
/// Fully transparent pixels are canonicalized to `[0, 0, 0, 0]`, so cached,
/// previewed, and exported images cannot carry invisible color fringes.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, mut data: Vec<u8>) -> Self {
        Self::canonicalize_transparent_rgb(&mut data);
        Self {
            width,
            height,
            data,
        }
    }

    pub fn canonicalize_transparent_rgb(data: &mut [u8]) {
        for pixel in data.chunks_exact_mut(4) {
            if pixel[3] == 0 {
                pixel[0] = 0;
                pixel[1] = 0;
                pixel[2] = 0;
            }
        }
    }
}
