use crate::error::LibraryError;
use crate::plugin::{EffectPlugin, Plugin};
use crate::project::property::PropertyValue;
use crate::runtime::Image;
use image::Rgba;
use log::debug;
use rayon::prelude::*;
use std::collections::HashMap; // Add rayon prelude

pub struct PixelSorterPlugin;

impl PixelSorterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for PixelSorterPlugin {
    fn id(&self) -> &'static str {
        "pixel_sorter"
    }

    fn name(&self) -> String {
        "Pixel Sorter".to_string()
    }

    fn category(&self) -> String {
        "Glitch".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for PixelSorterPlugin {
    fn apply(
        &self,
        input: &crate::rendering::renderer::RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut crate::rendering::skia_utils::GpuContext>,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        let threshold_value = params
            .get("threshold")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.5); // Default threshold

        debug!(
            "PixelSorterPlugin: Applying with threshold_value = {}",
            threshold_value
        );

        let direction_str = params
            .get("direction")
            .and_then(|pv| pv.get_as::<String>())
            .unwrap_or_else(|| {
                debug!(
                    "PixelSorterPlugin: 'direction' parameter not found, defaulting to 'horizontal'"
                );
                "horizontal".to_string()
            });

        let sort_criteria_str = params
            .get("sort_criteria")
            .and_then(|pv| pv.get_as::<String>())
            .unwrap_or_else(|| {
                debug!("PixelSorterPlugin: 'sort_criteria' parameter not found, defaulting to 'brightness'");
                "brightness".to_string()
            });

        // Resolve Image
        let image = match input {
            crate::rendering::renderer::RenderOutput::Image(img) => img.clone(),
            crate::rendering::renderer::RenderOutput::Texture(info) => {
                if let Some(ctx) = gpu_context {
                    let sk_image = crate::rendering::skia_utils::create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?;
                    let row_bytes = (info.width * 4) as usize;
                    let mut buffer = vec![0u8; (info.height as usize) * row_bytes];
                    let image_info = skia_safe::ImageInfo::new(
                        skia_safe::ISize::new(info.width as i32, info.height as i32),
                        skia_safe::ColorType::RGBA8888,
                        skia_safe::AlphaType::Premul,
                        None,
                    );
                    if !sk_image.read_pixels(
                        &image_info,
                        &mut buffer,
                        row_bytes,
                        (0, 0),
                        skia_safe::image::CachingHint::Disallow,
                    ) {
                        return Err(LibraryError::render(
                            "Failed to read texture pixels".to_string(),
                        ));
                    }
                    Image {
                        width: info.width,
                        height: info.height,
                        data: buffer,
                    }
                } else {
                    return Err(LibraryError::render(
                        "Cannot read texture without GPU context".to_string(),
                    ));
                }
            }
        };

        let mut processed_data = image.data.clone(); // Start with a mutable copy of the original data

        match direction_str.as_str() {
            "horizontal" => {
                processed_data
                    .par_chunks_mut((image.width * 4) as usize)
                    .for_each(|row_chunk| {
                        // Extract pixels for the row
                        let mut row_pixels: Vec<(u8, Rgba<u8>)> = row_chunk
                            .chunks_exact(4)
                            .map(|chunk| {
                                let pixel = Rgba([chunk[0], chunk[1], chunk[2], chunk[3]]);
                                (get_pixel_criteria_value(&pixel, &sort_criteria_str), pixel)
                            })
                            .collect();

                        // Apply sorting logic
                        let mut current_run_start: Option<usize> = None;
                        for x in 0..row_pixels.len() {
                            let (criteria_value, _) = row_pixels[x];
                            let pixel_norm = criteria_value as f32 / 255.0;

                            if pixel_norm < threshold_value as f32 {
                                // Condition met
                                if current_run_start.is_none() {
                                    current_run_start = Some(x); // Start a new run
                                }
                            } else {
                                // Condition not met
                                if let Some(start) = current_run_start {
                                    // End of run, sort the segment
                                    row_pixels[start..x].sort_by_key(|(val, _)| *val);
                                    current_run_start = None; // Reset for next run
                                }
                            }
                        }
                        // Handle any pending run at the end of the row
                        if let Some(start) = current_run_start {
                            let end_index = row_pixels.len(); // Store length first
                            row_pixels[start..end_index].sort_by_key(|(val, _)| *val);
                        }

                        // Write sorted pixels back to the row_chunk
                        for (x, (_, pixel)) in row_pixels.into_iter().enumerate() {
                            let start_index = x * 4;
                            row_chunk[start_index] = pixel[0];
                            row_chunk[start_index + 1] = pixel[1];
                            row_chunk[start_index + 2] = pixel[2];
                            row_chunk[start_index + 3] = pixel[3];
                        }
                    });
            }
            "vertical" => {
                // Convert to columns for vertical processing
                let mut columns: Vec<Vec<Rgba<u8>>> = (0..image.width)
                    .map(|x| {
                        (0..image.height)
                            .map(|y| {
                                let start_index = (y * image.width + x) as usize * 4;
                                Rgba([
                                    processed_data[start_index],
                                    processed_data[start_index + 1],
                                    processed_data[start_index + 2],
                                    processed_data[start_index + 3],
                                ])
                            })
                            .collect()
                    })
                    .collect();

                // Process each column in parallel
                columns.par_iter_mut().for_each(|col_pixels| {
                    let mut column_pixels_with_criteria: Vec<(u8, Rgba<u8>)> = col_pixels
                        .iter()
                        .map(|p| (get_pixel_criteria_value(p, &sort_criteria_str), *p))
                        .collect();

                    let mut current_run_start: Option<usize> = None;
                    for y in 0..column_pixels_with_criteria.len() {
                        let (criteria_value, _) = column_pixels_with_criteria[y];
                        let pixel_norm = criteria_value as f32 / 255.0;

                        if pixel_norm < threshold_value as f32 {
                            // Condition met
                            if current_run_start.is_none() {
                                current_run_start = Some(y); // Start a new run
                            }
                        } else {
                            // Condition not met
                            if let Some(start) = current_run_start {
                                // End of run, sort the segment
                                column_pixels_with_criteria[start..y].sort_by_key(|(val, _)| *val);
                                current_run_start = None; // Reset for next run
                            }
                        }
                    }
                    // Handle any pending run at the end of the column
                    if let Some(start) = current_run_start {
                        let end_index = column_pixels_with_criteria.len(); // Store length first
                        column_pixels_with_criteria[start..end_index].sort_by_key(|(val, _)| *val);
                    }

                    // Write sorted pixels back to col_pixels
                    for (y, (_, pixel)) in column_pixels_with_criteria.into_iter().enumerate() {
                        col_pixels[y] = pixel;
                    }
                });

                // Write processed columns back to processed_data
                for x in 0..image.width {
                    for y in 0..image.height {
                        let pixel = columns[x as usize][y as usize];
                        let start_index = (y * image.width + x) as usize * 4;
                        processed_data[start_index] = pixel[0];
                        processed_data[start_index + 1] = pixel[1];
                        processed_data[start_index + 2] = pixel[2];
                        processed_data[start_index + 3] = pixel[3];
                    }
                }
            }
            _ => {
                return Err(LibraryError::plugin(format!(
                    "Unsupported sort direction: {}",
                    direction_str
                )));
            }
        }

        Ok(crate::rendering::renderer::RenderOutput::Image(Image {
            width: image.width,
            height: image.height,
            data: processed_data,
        }))
    }

    fn properties(&self) -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::PropertyValue;
        use crate::project::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "threshold",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.01,
                    suffix: "".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Threshold",
                PropertyValue::Number(OrderedFloat(0.5)),
            ),
            PropertyDefinition::new(
                "direction",
                PropertyUiType::Dropdown {
                    options: vec!["horizontal".to_string(), "vertical".to_string()],
                },
                "Direction",
                PropertyValue::String("horizontal".to_string()),
            ),
            PropertyDefinition::new(
                "sort_criteria",
                PropertyUiType::Dropdown {
                    options: vec![
                        "brightness".to_string(),
                        "red".to_string(),
                        "green".to_string(),
                        "blue".to_string(),
                    ],
                },
                "Criteria",
                PropertyValue::String("brightness".to_string()),
            ),
        ]
    }
}

fn get_pixel_criteria_value(pixel: &Rgba<u8>, sort_criteria: &str) -> u8 {
    match sort_criteria {
        "brightness" => {
            // Simple brightness calculation (average of RGB)
            ((pixel[0] as u16 + pixel[1] as u16 + pixel[2] as u16) / 3) as u8
        }
        "red" => pixel[0],
        "green" => pixel[1],
        "blue" => pixel[2],
        // Add more criteria like hue, saturation, value if needed
        _ => ((pixel[0] as u16 + pixel[1] as u16 + pixel[2] as u16) / 3) as u8, // Default to brightness
    }
}
