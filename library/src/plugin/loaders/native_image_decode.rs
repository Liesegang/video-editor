//! Bounded native still-image I/O and typed payload adoption.
//!
//! Metadata interpretation remains in `native_image`; this module owns the
//! point where a decoder is allowed to allocate. Both the decoder's current
//! dimensions and the final loader storage are validated before decoding or
//! RGBA conversion begins.

use super::{
    DecodedPixelBuffer, DecodedPixelLayout, DecodedPixelStorage, DecodedStraightRgba32F,
    validate_decoded_pixel_layout,
};
use crate::model::frame::Image;
use crate::util::local_file::DirectRegularFile;
use image::ImageDecoder;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

pub(super) fn guessed_reader(
    path: &str,
) -> Result<image::ImageReader<BufReader<File>>, Box<dyn Error>> {
    let mut reader = image::ImageReader::new(BufReader::new(open_regular(path)?));
    // TGA and a few legacy still formats have no reliable magic signature;
    // preserve the extension hint that `ImageReader::open` used to provide,
    // while `with_guessed_format` still handles renamed signature formats.
    if let Ok(format) = image::ImageFormat::from_path(path) {
        reader.set_format(format);
    }
    Ok(reader.with_guessed_format()?)
}

pub(super) fn open_regular(path: &str) -> Result<File, std::io::Error> {
    DirectRegularFile::open(path).map(DirectRegularFile::into_file)
}

/// Load an image from disk as a bounded straight RGBA8 payload.
pub fn load_image(path: &str) -> Result<Image, Box<dyn Error>> {
    let (image, _layout) = decode_image_with_target(path, DecodedPixelStorage::StraightRgba8)
        .map_err(|error| format!("Failed to open image file: {error}"))?;
    // Consuming conversion reuses an already-RGBA8 decoder allocation.
    let rgba_image = image.into_rgba8();
    Ok(Image::new(
        rgba_image.width(),
        rgba_image.height(),
        rgba_image.into_raw(),
    ))
}

pub(super) fn load_high_precision_image(path: &str) -> Result<DecodedPixelBuffer, Box<dyn Error>> {
    let (image, layout) = decode_image_with_target(path, DecodedPixelStorage::StraightRgba32F)
        .map_err(|error| format!("Failed to open high-precision image file: {error}"))?;
    // `into_rgba32f` can reuse a native RGBA32F decode. Convert its flat
    // component allocation to typed pixels without retaining a second copy of
    // what can be a 512 MiB payload. A boxed slice avoids Vec-capacity
    // divisibility constraints while keeping this a safe, zero-copy cast.
    let rgba = image.into_rgba32f();
    let pixels = bytemuck::allocation::try_cast_slice_box::<f32, [f32; 4]>(
        rgba.into_raw().into_boxed_slice(),
    )
    .map_err(|(error, _components)| format!("invalid RGBA32F component layout: {error}"))?
    .into_vec();
    if pixels.len() != layout.pixel_count() {
        return Err(format!(
            "decoded RGBA32F pixel count changed from {} to {}",
            layout.pixel_count(),
            pixels.len()
        )
        .into());
    }
    Ok(DecodedPixelBuffer::StraightRgba32F(
        DecodedStraightRgba32F::new(layout.width(), layout.height(), pixels)?,
    ))
}

fn decode_image_with_target(
    path: &str,
    storage: DecodedPixelStorage,
) -> Result<(image::DynamicImage, DecodedPixelLayout), Box<dyn Error>> {
    let reader = guessed_reader(path)?;
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    decode_after_layout_validation(width, height, storage, |layout| {
        Ok((image::DynamicImage::from_decoder(decoder)?, layout))
    })
}

fn decode_after_layout_validation<T>(
    width: u32,
    height: u32,
    storage: DecodedPixelStorage,
    decode: impl FnOnce(DecodedPixelLayout) -> Result<T, Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    let layout = validate_decoded_pixel_layout(width, height, storage)?;
    decode(layout)
}

#[cfg(test)]
mod tests {
    use super::decode_after_layout_validation;
    use crate::plugin::loaders::DecodedPixelStorage;
    use std::cell::Cell;

    #[test]
    fn oversized_targets_are_rejected_before_decoder_callback() {
        for storage in [
            DecodedPixelStorage::StraightRgba8,
            DecodedPixelStorage::StraightRgba32F,
        ] {
            let attempted = Cell::new(false);
            let result = decode_after_layout_validation(
                65_536,
                65_536,
                storage,
                |_| -> Result<(), Box<dyn std::error::Error>> {
                    attempted.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err(), "oversized {storage:?} target was accepted");
            assert!(
                !attempted.get(),
                "native decoder callback ran for oversized {storage:?} target"
            );
        }
    }
}
