use super::*;

fn pixel(key: u8, marker: u8, alpha: u8) -> [u8; 4] {
    [key, marker, 255_u8.wrapping_sub(marker), alpha]
}

fn pixels(values: &[[u8; 4]]) -> Vec<u8> {
    values.iter().flatten().copied().collect()
}

fn options(
    threshold: f64,
    direction: &str,
    criteria: &str,
) -> Result<PixelSortOptions, PixelSortError> {
    PixelSortOptions::parse(threshold, direction, criteria)
}

fn sorted(
    width: u32,
    height: u32,
    input: &[u8],
    options: PixelSortOptions,
) -> Result<Vec<u8>, PixelSortError> {
    sort_rgba8(width, height, input, options)
}

#[test]
fn horizontal_golden_covers_multiple_and_trailing_runs() -> Result<(), PixelSortError> {
    let input = pixels(&[
        pixel(30, 1, 11),
        pixel(10, 2, 22),
        pixel(200, 3, 33),
        pixel(90, 4, 44),
        pixel(20, 5, 55),
    ]);
    let expected = pixels(&[
        pixel(10, 2, 22),
        pixel(30, 1, 11),
        pixel(200, 3, 33),
        pixel(20, 5, 55),
        pixel(90, 4, 44),
    ]);

    assert_eq!(
        sorted(5, 1, &input, options(0.5, "horizontal", "red")?)?,
        expected
    );
    Ok(())
}

#[test]
fn vertical_golden_sorts_columns_without_crossing_rows() -> Result<(), PixelSortError> {
    let input = pixels(&[
        pixel(30, 1, 11),
        pixel(7, 10, 101),
        pixel(10, 2, 22),
        pixel(200, 11, 102),
        pixel(200, 3, 33),
        pixel(5, 12, 103),
        pixel(90, 4, 44),
        pixel(3, 13, 104),
        pixel(20, 5, 55),
        pixel(250, 14, 105),
    ]);
    let expected = pixels(&[
        pixel(10, 2, 22),
        pixel(7, 10, 101),
        pixel(30, 1, 11),
        pixel(200, 11, 102),
        pixel(200, 3, 33),
        pixel(3, 13, 104),
        pixel(20, 5, 55),
        pixel(5, 12, 103),
        pixel(90, 4, 44),
        pixel(250, 14, 105),
    ]);

    assert_eq!(
        sorted(2, 5, &input, options(0.5, "vertical", "red")?)?,
        expected
    );
    Ok(())
}

#[test]
fn zero_threshold_leaves_empty_runs_unchanged() -> Result<(), PixelSortError> {
    let input = pixels(&[pixel(2, 1, 10), pixel(1, 2, 20), pixel(0, 3, 30)]);
    assert_eq!(
        sorted(3, 1, &input, options(0.0, "horizontal", "red")?)?,
        input
    );
    Ok(())
}

#[test]
fn equal_keys_keep_original_axis_order_and_alpha_with_their_pixels() -> Result<(), PixelSortError> {
    let input = pixels(&[
        pixel(40, 1, 9),
        pixel(10, 2, 37),
        pixel(10, 3, 91),
        pixel(20, 4, 173),
    ]);
    let expected = pixels(&[
        pixel(10, 2, 37),
        pixel(10, 3, 91),
        pixel(20, 4, 173),
        pixel(40, 1, 9),
    ]);

    assert_eq!(
        sorted(4, 1, &input, options(1.0, "horizontal", "red")?)?,
        expected
    );
    Ok(())
}

#[test]
fn strict_threshold_excludes_the_boundary_key() -> Result<(), PixelSortError> {
    let boundary = f64::from(128_f32 / 255.0);
    let input = pixels(&[pixel(128, 1, 10), pixel(1, 2, 20)]);

    assert_eq!(
        sorted(2, 1, &input, options(boundary, "horizontal", "red")?)?,
        input
    );
    Ok(())
}

#[test]
fn rejects_invalid_layout_and_options_with_specific_errors() -> Result<(), PixelSortError> {
    assert!(matches!(
        sort_rgba8(0, 1, &[], options(0.5, "horizontal", "brightness")?),
        Err(PixelSortError::EmptyDimensions(0, 1))
    ));
    assert!(matches!(
        sort_rgba8(
            MAX_CPU_RGBA8_DIMENSION_V1 + 1,
            1,
            &[],
            options(0.5, "horizontal", "brightness")?
        ),
        Err(PixelSortError::DimensionsTooLarge(_, 1))
    ));
    assert!(matches!(
        rgba8_buffer_layout(MAX_CPU_RGBA8_DIMENSION_V1, MAX_CPU_RGBA8_DIMENSION_V1),
        Err(PixelSortError::FrameTooLarge(_))
    ));
    assert!(matches!(
        sort_rgba8(2, 2, &[0; 15], options(0.5, "horizontal", "brightness")?),
        Err(PixelSortError::BufferLength {
            expected: 16,
            actual: 15,
            ..
        })
    ));
    assert!(matches!(
        PixelSortOptions::parse(f64::NAN, "horizontal", "red"),
        Err(PixelSortError::InvalidThreshold(value)) if value.is_nan()
    ));
    assert!(matches!(
        PixelSortOptions::parse(-0.01, "horizontal", "red"),
        Err(PixelSortError::InvalidThreshold(-0.01))
    ));
    assert!(matches!(
        PixelSortOptions::parse(1.01, "horizontal", "red"),
        Err(PixelSortError::InvalidThreshold(1.01))
    ));
    assert_eq!(
        PixelSortOptions::parse(0.5, "diagonal", "red"),
        Err(PixelSortError::UnsupportedDirection("diagonal".to_string()))
    );
    assert_eq!(
        PixelSortOptions::parse(0.5, "horizontal", "luma"),
        Err(PixelSortError::UnsupportedCriteria("luma".to_string()))
    );
    Ok(())
}

#[test]
fn randomized_outputs_match_the_preserved_reference_algorithm() -> Result<(), PixelSortError> {
    let mut rng = DeterministicRng::new(0x44c1_9a27_befd_1001);
    let directions = [SortDirection::Horizontal, SortDirection::Vertical];
    let criteria = [
        SortCriteria::Brightness,
        SortCriteria::Red,
        SortCriteria::Green,
        SortCriteria::Blue,
    ];
    let thresholds = [0.0_f32, 0.2, 0.5, 128.0 / 255.0, 0.9, 1.0];

    for case_index in 0..96 {
        let width = usize::from(rng.next_u8() % 8 + 1);
        let height = usize::from(rng.next_u8() % 8 + 1);
        let mut input = vec![0; width * height * RGBA8_BYTES_PER_PIXEL];
        for byte in &mut input {
            *byte = rng.next_u8();
        }

        for direction in directions {
            for criterion in criteria {
                let threshold = thresholds[case_index % thresholds.len()];
                let options = PixelSortOptions {
                    threshold,
                    direction,
                    criteria: criterion,
                };
                let expected = preserved_reference(width, height, &input, options);
                let actual = sorted(width as u32, height as u32, &input, options)?;
                assert_eq!(
                    actual, expected,
                    "case {case_index}, {width}x{height}, {direction:?}, {criterion:?}, threshold={threshold}"
                );
            }
        }
    }
    Ok(())
}

fn preserved_reference(
    width: usize,
    height: usize,
    input: &[u8],
    options: PixelSortOptions,
) -> Vec<u8> {
    let mut output = input.to_vec();
    match options.direction {
        SortDirection::Horizontal => {
            for row in output.chunks_exact_mut(width * RGBA8_BYTES_PER_PIXEL) {
                let mut line = legacy_line(row, options.criteria);
                legacy_sort_runs(&mut line, options.threshold);
                write_legacy_contiguous(row, &line);
            }
        }
        SortDirection::Vertical => {
            let mut columns: Vec<Vec<(u8, [u8; 4])>> = (0..width)
                .map(|x| {
                    (0..height)
                        .map(|y| {
                            let offset = (y * width + x) * RGBA8_BYTES_PER_PIXEL;
                            let rgba = [
                                output[offset],
                                output[offset + 1],
                                output[offset + 2],
                                output[offset + 3],
                            ];
                            (options.criteria.value(rgba), rgba)
                        })
                        .collect()
                })
                .collect();
            for column in &mut columns {
                legacy_sort_runs(column, options.threshold);
            }
            for (x, column) in columns.iter().enumerate() {
                for (y, (_, rgba)) in column.iter().enumerate() {
                    let offset = (y * width + x) * RGBA8_BYTES_PER_PIXEL;
                    output[offset..offset + RGBA8_BYTES_PER_PIXEL].copy_from_slice(rgba);
                }
            }
        }
    }
    output
}

fn legacy_line(line: &[u8], criteria: SortCriteria) -> Vec<(u8, [u8; 4])> {
    line.chunks_exact(RGBA8_BYTES_PER_PIXEL)
        .map(|pixel| {
            let rgba = [pixel[0], pixel[1], pixel[2], pixel[3]];
            (criteria.value(rgba), rgba)
        })
        .collect()
}

fn legacy_sort_runs(line: &mut [(u8, [u8; 4])], threshold: f32) {
    let mut run_start = None;
    for index in 0..line.len() {
        if f32::from(line[index].0) / 255.0 < threshold {
            run_start.get_or_insert(index);
        } else if let Some(start) = run_start.take() {
            line[start..index].sort_by_key(|(value, _)| *value);
        }
    }
    if let Some(start) = run_start {
        line[start..].sort_by_key(|(value, _)| *value);
    }
}

fn write_legacy_contiguous(output: &mut [u8], line: &[(u8, [u8; 4])]) {
    for (target, (_, rgba)) in output.chunks_exact_mut(RGBA8_BYTES_PER_PIXEL).zip(line) {
        target.copy_from_slice(rgba);
    }
}

struct DeterministicRng(u64);

impl DeterministicRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u8(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0.to_be_bytes()[0]
    }
}
