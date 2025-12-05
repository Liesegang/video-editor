use log::debug;
use std::collections::HashMap;
use image::{ImageBuffer, Rgba};
use crate::loader::image::Image;
use crate::error::LibraryError;
use crate::plugin::{Plugin, PluginCategory, EffectPlugin};
use crate::model::project::property::PropertyValue;
use rayon::prelude::*; // Add rayon prelude

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

    fn category(&self) -> PluginCategory {
        PluginCategory::Effect
    }
}

impl EffectPlugin for PixelSorterPlugin {
    fn apply(&self, image: &Image, params: &HashMap<String, PropertyValue>) -> Result<Image, LibraryError> {
        let threshold_value = params
            .get("threshold")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.5); // Default threshold

        debug!("PixelSorterPlugin: Applying with threshold_value = {}", threshold_value);

        let direction_str = params
            .get("direction")
            .and_then(|pv| pv.get_as::<String>())
            .unwrap_or_else(|| {
                debug!("PixelSorterPlugin: 'direction' parameter not found, defaulting to 'horizontal'");
                "horizontal".to_string()
            });

        let sort_criteria_str = params
            .get("sort_criteria")
            .and_then(|pv| pv.get_as::<String>())
            .unwrap_or_else(|| {
                debug!("PixelSorterPlugin: 'sort_criteria' parameter not found, defaulting to 'brightness'");
                "brightness".to_string()
            });

        let mut processed_data = image.data.clone(); // Start with a mutable copy of the original data

        match direction_str.as_str() {
            "horizontal" => {
                processed_data.par_chunks_mut((image.width * 4) as usize)
                    .for_each(|row_chunk| {
                        // Extract pixels for the row
                        let mut row_pixels: Vec<(u8, Rgba<u8>)> = row_chunk.chunks_exact(4).map(|chunk| {
                            let pixel = Rgba([chunk[0], chunk[1], chunk[2], chunk[3]]);
                            (get_pixel_criteria_value(&pixel, &sort_criteria_str), pixel)
                        }).collect();

                        // Apply sorting logic
                        let mut current_run_start: Option<usize> = None;
                        for x in 0..row_pixels.len() {
                            let (criteria_value, _) = row_pixels[x];
                            let pixel_norm = criteria_value as f32 / 255.0;

                            if pixel_norm < threshold_value as f32 { // Condition met
                                if current_run_start.is_none() {
                                    current_run_start = Some(x); // Start a new run
                                }
                            } else { // Condition not met
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
                                Rgba([processed_data[start_index], processed_data[start_index + 1], processed_data[start_index + 2], processed_data[start_index + 3]])
                            })
                            .collect()
                    })
                    .collect();

                // Process each column in parallel
                columns.par_iter_mut().for_each(|col_pixels| {
                    let mut column_pixels_with_criteria: Vec<(u8, Rgba<u8>)> = col_pixels.iter().map(|p| {
                        (get_pixel_criteria_value(p, &sort_criteria_str), *p)
                    }).collect();

                    let mut current_run_start: Option<usize> = None;
                    for y in 0..column_pixels_with_criteria.len() {
                        let (criteria_value, _) = column_pixels_with_criteria[y];
                        let pixel_norm = criteria_value as f32 / 255.0;

                        if pixel_norm < threshold_value as f32 { // Condition met
                            if current_run_start.is_none() {
                                current_run_start = Some(y); // Start a new run
                            }
                        } else { // Condition not met
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
            _ => return Err(LibraryError::Plugin(format!("Unsupported sort direction: {}", direction_str))),
        }

        Ok(Image {
            width: image.width,
            height: image.height,
            data: processed_data,
        })
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