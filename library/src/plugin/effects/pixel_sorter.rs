use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin};
use log::debug;
use std::collections::HashMap;

pub(crate) mod kernel;

use kernel::{PixelSortOptions, rgba8_buffer_layout, sort_rgba8};

#[derive(Default)]
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
        let threshold_value = numeric_parameter(params, "threshold", 0.5)?;

        debug!(
            "PixelSorterPlugin: Applying with threshold_value = {}",
            threshold_value
        );

        let direction = string_parameter(params, "direction", "horizontal")?;
        let sort_criteria = string_parameter(params, "sort_criteria", "brightness")?;
        let options = PixelSortOptions::parse(threshold_value, &direction, &sort_criteria)
            .map_err(|error| LibraryError::Plugin(error.to_string()))?;

        let (width, height, input_data) = match input {
            crate::rendering::renderer::RenderOutput::Image(image) => {
                (image.width, image.height, image.data.as_slice())
            }
            crate::rendering::renderer::RenderOutput::Texture(info) => {
                if let Some(ctx) = gpu_context {
                    let sk_image = crate::rendering::skia_utils::create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?;
                    let (row_bytes, frame_bytes) = rgba8_buffer_layout(info.width, info.height)
                        .map_err(|error| LibraryError::Plugin(error.to_string()))?;
                    let mut buffer = vec![0u8; frame_bytes];
                    let image_info = skia_safe::ImageInfo::new(
                        skia_safe::ISize::new(info.width as i32, info.height as i32),
                        skia_safe::ColorType::RGBA8888,
                        skia_safe::AlphaType::Unpremul,
                        None,
                    );
                    if !sk_image.read_pixels(
                        &image_info,
                        &mut buffer,
                        row_bytes,
                        (0, 0),
                        skia_safe::image::CachingHint::Disallow,
                    ) {
                        return Err(LibraryError::Render(
                            "Failed to read texture pixels".to_string(),
                        ));
                    }
                    // The former adapter constructed an Image before sorting,
                    // so transparent RGB was canonicalized before criteria
                    // evaluation. Preserve that texture-input behavior.
                    Image::canonicalize_transparent_rgb(&mut buffer);
                    let processed_data = sort_rgba8(info.width, info.height, &buffer, options)
                        .map_err(|error| LibraryError::Plugin(error.to_string()))?;
                    return Ok(crate::rendering::renderer::RenderOutput::Image(Image::new(
                        info.width,
                        info.height,
                        processed_data,
                    )));
                } else {
                    return Err(LibraryError::Render(
                        "Cannot read texture without GPU context".to_string(),
                    ));
                }
            }
        };

        let processed_data = sort_rgba8(width, height, input_data, options)
            .map_err(|error| LibraryError::Plugin(error.to_string()))?;

        Ok(crate::rendering::renderer::RenderOutput::Image(Image::new(
            width,
            height,
            processed_data,
        )))
    }

    fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::PropertyValue;
        use crate::model::property::{PropertyDefinition, PropertyUiType};
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

fn numeric_parameter(
    params: &HashMap<String, PropertyValue>,
    name: &str,
    default: f64,
) -> Result<f64, LibraryError> {
    match params.get(name) {
        Some(value) => value.get_as::<f64>().ok_or_else(|| {
            LibraryError::Plugin(format!("pixel sorter parameter {name:?} must be a number"))
        }),
        None => Ok(default),
    }
}

fn string_parameter(
    params: &HashMap<String, PropertyValue>,
    name: &str,
    default: &str,
) -> Result<String, LibraryError> {
    match params.get(name) {
        Some(value) => value.get_as::<String>().ok_or_else(|| {
            LibraryError::Plugin(format!("pixel sorter parameter {name:?} must be a string"))
        }),
        None => {
            debug!("PixelSorterPlugin: {name:?} parameter not found, defaulting to {default:?}");
            Ok(default.to_string())
        }
    }
}
