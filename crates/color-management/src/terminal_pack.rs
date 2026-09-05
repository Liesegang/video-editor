use rayon::prelude::*;

use crate::LinearWorkingImageError;

pub(crate) const TERMINAL_PARALLEL_PIXEL_THRESHOLD: usize = 4 * 1024;

pub(crate) fn pack_straight_rgba8(
    width: u32,
    height: u32,
    straight: &[[f32; 4]],
) -> Result<Vec<u8>, LinearWorkingImageError> {
    let mut rgba = allocate_rgba8(width, height, straight.len())?;
    if straight.len() < TERMINAL_PARALLEL_PIXEL_THRESHOLD {
        for (packed, pixel) in rgba.chunks_exact_mut(4).zip(straight) {
            pack_pixel(packed, [pixel[0], pixel[1], pixel[2]], pixel[3]);
        }
    } else {
        rgba.par_chunks_exact_mut(4)
            .zip(straight.par_iter())
            .for_each(|(packed, pixel)| {
                pack_pixel(packed, [pixel[0], pixel[1], pixel[2]], pixel[3]);
            });
    }
    Ok(rgba)
}

pub(crate) fn pack_transformed_straight_rgba8(
    width: u32,
    height: u32,
    straight: &[[f32; 3]],
    premultiplied: &[[f32; 4]],
) -> Result<Vec<u8>, LinearWorkingImageError> {
    debug_assert_eq!(straight.len(), premultiplied.len());
    let mut rgba = allocate_rgba8(width, height, straight.len())?;
    if straight.len() < TERMINAL_PARALLEL_PIXEL_THRESHOLD {
        for ((packed, rgb), source) in rgba.chunks_exact_mut(4).zip(straight).zip(premultiplied) {
            let alpha = source[3];
            pack_pixel(packed, if alpha == 0.0 { [0.0; 3] } else { *rgb }, alpha);
        }
    } else {
        rgba.par_chunks_exact_mut(4)
            .zip(straight.par_iter().zip(premultiplied.par_iter()))
            .for_each(|(packed, (rgb, source))| {
                let alpha = source[3];
                pack_pixel(packed, if alpha == 0.0 { [0.0; 3] } else { *rgb }, alpha);
            });
    }
    Ok(rgba)
}

fn allocate_rgba8(
    width: u32,
    height: u32,
    pixel_count: usize,
) -> Result<Vec<u8>, LinearWorkingImageError> {
    let component_count = pixel_count
        .checked_mul(4)
        .ok_or(LinearWorkingImageError::DimensionOverflow { width, height })?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(component_count).map_err(|_| {
        LinearWorkingImageError::AllocationFailed {
            bytes: component_count,
        }
    })?;
    rgba.resize(component_count, 0);
    Ok(rgba)
}

fn pack_pixel(destination: &mut [u8], rgb: [f32; 3], alpha: f32) {
    destination.copy_from_slice(&[
        quantize_unorm8(f64::from(rgb[0])),
        quantize_unorm8(f64::from(rgb[1])),
        quantize_unorm8(f64::from(rgb[2])),
        quantize_unorm8(f64::from(alpha)),
    ]);
}

fn quantize_unorm8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
