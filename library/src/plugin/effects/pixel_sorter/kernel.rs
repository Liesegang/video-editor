//! Deterministic CPU reference implementation for pixel sorting.
//!
//! The renderer adapter deliberately lives outside this module. Keeping this
//! kernel pure gives a future GPU implementation a small, testable oracle.

use std::sync::{Mutex, PoisonError};

use rayon::prelude::*;
use ruvie_plugin_api::{MAX_CPU_RGBA8_DIMENSION_V1, MAX_CPU_RGBA8_FRAME_BYTES_V1};

const RGBA8_BYTES_PER_PIXEL: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortDirection {
    Horizontal,
    Vertical,
}

impl SortDirection {
    fn parse(value: &str) -> Result<Self, PixelSortError> {
        match value {
            "horizontal" => Ok(Self::Horizontal),
            "vertical" => Ok(Self::Vertical),
            _ => Err(PixelSortError::UnsupportedDirection(value.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortCriteria {
    Brightness,
    Red,
    Green,
    Blue,
}

impl SortCriteria {
    fn parse(value: &str) -> Result<Self, PixelSortError> {
        match value {
            "brightness" => Ok(Self::Brightness),
            "red" => Ok(Self::Red),
            "green" => Ok(Self::Green),
            "blue" => Ok(Self::Blue),
            _ => Err(PixelSortError::UnsupportedCriteria(value.to_string())),
        }
    }

    fn value(self, rgba: [u8; RGBA8_BYTES_PER_PIXEL]) -> u8 {
        match self {
            Self::Brightness => {
                let sum = u16::from(rgba[0]) + u16::from(rgba[1]) + u16::from(rgba[2]);
                u8::try_from(sum / 3).unwrap_or(u8::MAX)
            }
            Self::Red => rgba[0],
            Self::Green => rgba[1],
            Self::Blue => rgba[2],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PixelSortOptions {
    threshold: f32,
    direction: SortDirection,
    criteria: SortCriteria,
}

impl PixelSortOptions {
    pub(crate) fn parse(
        threshold: f64,
        direction: &str,
        criteria: &str,
    ) -> Result<Self, PixelSortError> {
        if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
            return Err(PixelSortError::InvalidThreshold(threshold));
        }

        Ok(Self {
            // The original implementation compared using f32. Keeping the
            // narrowing conversion here preserves threshold boundary behavior.
            threshold: threshold as f32,
            direction: SortDirection::parse(direction)?,
            criteria: SortCriteria::parse(criteria)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub(crate) enum PixelSortError {
    #[error("pixel sorter dimensions must be non-zero, got {0}x{1}")]
    EmptyDimensions(u32, u32),
    #[error(
        "pixel sorter dimensions {0}x{1} exceed the supported maximum of {MAX_CPU_RGBA8_DIMENSION_V1} per axis"
    )]
    DimensionsTooLarge(u32, u32),
    #[error("pixel sorter RGBA8 layout overflows for dimensions {0}x{1}")]
    LayoutOverflow(u32, u32),
    #[error(
        "pixel sorter RGBA8 buffer length {actual} does not match {width}x{height} layout ({expected} bytes)"
    )]
    BufferLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
    #[error(
        "pixel sorter RGBA8 layout requires {0} bytes, above the supported maximum of {MAX_CPU_RGBA8_FRAME_BYTES_V1}"
    )]
    FrameTooLarge(usize),
    #[error("pixel sorter threshold must be finite and in 0.0..=1.0, got {0}")]
    InvalidThreshold(f64),
    #[error("unsupported pixel sorter direction {0:?}; expected \"horizontal\" or \"vertical\"")]
    UnsupportedDirection(String),
    #[error(
        "unsupported pixel sorter criteria {0:?}; expected \"brightness\", \"red\", \"green\", or \"blue\""
    )]
    UnsupportedCriteria(String),
}

#[derive(Clone, Copy, Debug)]
struct SortablePixel {
    criteria: u8,
    original_axis_index: usize,
    rgba: [u8; RGBA8_BYTES_PER_PIXEL],
}

/// Sort straight RGBA8 pixels into a newly allocated output buffer.
///
/// For valid inputs this performs exactly one full-frame clone. Additional
/// memory is bounded by one axis-sized scratch vector per active Rayon worker.
pub(crate) fn sort_rgba8(
    width: u32,
    height: u32,
    input: &[u8],
    options: PixelSortOptions,
) -> Result<Vec<u8>, PixelSortError> {
    let layout = ValidatedLayout::with_buffer(width, height, input.len())?;
    let mut output = input.to_vec();

    match options.direction {
        SortDirection::Horizontal => {
            sort_horizontal(&mut output, layout.row_bytes, options);
        }
        SortDirection::Vertical => {
            output = sort_vertical(input, output, layout, options);
        }
    }

    Ok(output)
}

#[derive(Clone, Copy, Debug)]
struct ValidatedLayout {
    width: usize,
    height: usize,
    row_bytes: usize,
    frame_bytes: usize,
}

impl ValidatedLayout {
    fn dimensions(width: u32, height: u32) -> Result<Self, PixelSortError> {
        if width == 0 || height == 0 {
            return Err(PixelSortError::EmptyDimensions(width, height));
        }
        if width > MAX_CPU_RGBA8_DIMENSION_V1 || height > MAX_CPU_RGBA8_DIMENSION_V1 {
            return Err(PixelSortError::DimensionsTooLarge(width, height));
        }

        let width_usize =
            usize::try_from(width).map_err(|_| PixelSortError::LayoutOverflow(width, height))?;
        let height_usize =
            usize::try_from(height).map_err(|_| PixelSortError::LayoutOverflow(width, height))?;
        let row_bytes = width_usize
            .checked_mul(RGBA8_BYTES_PER_PIXEL)
            .ok_or(PixelSortError::LayoutOverflow(width, height))?;
        let frame_bytes = row_bytes
            .checked_mul(height_usize)
            .ok_or(PixelSortError::LayoutOverflow(width, height))?;

        if frame_bytes > MAX_CPU_RGBA8_FRAME_BYTES_V1 {
            return Err(PixelSortError::FrameTooLarge(frame_bytes));
        }

        Ok(Self {
            width: width_usize,
            height: height_usize,
            row_bytes,
            frame_bytes,
        })
    }

    fn with_buffer(width: u32, height: u32, actual_length: usize) -> Result<Self, PixelSortError> {
        let layout = Self::dimensions(width, height)?;
        if actual_length != layout.frame_bytes {
            return Err(PixelSortError::BufferLength {
                width,
                height,
                expected: layout.frame_bytes,
                actual: actual_length,
            });
        }
        Ok(layout)
    }
}

pub(crate) fn rgba8_buffer_layout(
    width: u32,
    height: u32,
) -> Result<(usize, usize), PixelSortError> {
    let layout = ValidatedLayout::dimensions(width, height)?;
    Ok((layout.row_bytes, layout.frame_bytes))
}

fn sort_horizontal(output: &mut [u8], row_bytes: usize, options: PixelSortOptions) {
    output.par_chunks_mut(row_bytes).for_each_init(
        Vec::new,
        |scratch: &mut Vec<SortablePixel>, row| {
            load_contiguous_line(scratch, row, options.criteria);
            sort_runs(scratch, options.threshold);
            write_contiguous_line(row, scratch);
        },
    );
}

fn sort_vertical(
    input: &[u8],
    output: Vec<u8>,
    layout: ValidatedLayout,
    options: PixelSortOptions,
) -> Vec<u8> {
    // Columns are disjoint but not contiguous in row-major storage. A mutex
    // keeps the scatter safe while sorting remains parallel. Each Rayon worker
    // reuses one height-sized scratch vector; no whole-image transpose exists.
    let output = Mutex::new(output);
    (0..layout.width).into_par_iter().for_each_init(
        Vec::new,
        |scratch: &mut Vec<SortablePixel>, x| {
            load_vertical_line(scratch, input, x, layout, options.criteria);
            sort_runs(scratch, options.threshold);

            let mut output_guard = output.lock().unwrap_or_else(PoisonError::into_inner);
            write_vertical_line(&mut output_guard, scratch, x, layout.row_bytes);
        },
    );

    match output.into_inner() {
        Ok(output) => output,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn load_contiguous_line(scratch: &mut Vec<SortablePixel>, line: &[u8], criteria: SortCriteria) {
    scratch.clear();
    scratch.extend(line.chunks_exact(RGBA8_BYTES_PER_PIXEL).enumerate().map(
        |(original_axis_index, pixel)| {
            let rgba = [pixel[0], pixel[1], pixel[2], pixel[3]];
            SortablePixel {
                criteria: criteria.value(rgba),
                original_axis_index,
                rgba,
            }
        },
    ));
}

fn load_vertical_line(
    scratch: &mut Vec<SortablePixel>,
    input: &[u8],
    x: usize,
    layout: ValidatedLayout,
    criteria: SortCriteria,
) {
    scratch.clear();
    scratch.reserve(layout.height);
    for original_axis_index in 0..layout.height {
        let offset = original_axis_index * layout.row_bytes + x * RGBA8_BYTES_PER_PIXEL;
        let rgba = [
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
        ];
        scratch.push(SortablePixel {
            criteria: criteria.value(rgba),
            original_axis_index,
            rgba,
        });
    }
}

fn sort_runs(line: &mut [SortablePixel], threshold: f32) {
    let mut run_start = None;
    for index in 0..line.len() {
        // This strict predicate intentionally matches the original effect.
        let belongs_to_run = f32::from(line[index].criteria) / 255.0 < threshold;
        if belongs_to_run {
            run_start.get_or_insert(index);
        } else if let Some(start) = run_start.take() {
            sort_run(&mut line[start..index]);
        }
    }
    if let Some(start) = run_start {
        sort_run(&mut line[start..]);
    }
}

fn sort_run(run: &mut [SortablePixel]) {
    // The source-axis index makes the old stable-sort behavior explicit and
    // deterministic even if the sorting implementation changes later.
    run.sort_by_key(|pixel| (pixel.criteria, pixel.original_axis_index));
}

fn write_contiguous_line(output: &mut [u8], line: &[SortablePixel]) {
    for (target, pixel) in output.chunks_exact_mut(RGBA8_BYTES_PER_PIXEL).zip(line) {
        target.copy_from_slice(&pixel.rgba);
    }
}

fn write_vertical_line(output: &mut [u8], line: &[SortablePixel], x: usize, row_bytes: usize) {
    for (y, pixel) in line.iter().enumerate() {
        let offset = y * row_bytes + x * RGBA8_BYTES_PER_PIXEL;
        output[offset..offset + RGBA8_BYTES_PER_PIXEL].copy_from_slice(&pixel.rgba);
    }
}
