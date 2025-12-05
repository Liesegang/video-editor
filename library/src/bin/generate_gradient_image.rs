use image::{ImageBuffer, Rgba};

fn main() {
    let width = 1280;
    let height = 720;

    let mut img = ImageBuffer::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let gray_value = (x as f32 / width as f32 * 255.0) as u8;
            let pixel = Rgba([gray_value, gray_value, gray_value, 255]);
            img.put_pixel(x, y, pixel);
        }
    }

    // Save the image
    img.save("test_data/gradient.png").unwrap();
    println!("Generated test_data/gradient.png");
}
