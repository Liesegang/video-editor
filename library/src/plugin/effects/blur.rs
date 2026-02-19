use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{TileMode, image_filters};
use std::collections::HashMap;

pub struct BlurEffectPlugin;

impl BlurEffectPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for BlurEffectPlugin {
    fn id(&self) -> &'static str {
        "blur"
    }

    fn name(&self) -> String {
        "Blur".to_string()
    }

    fn category(&self) -> String {
        "Blur & Sharpen".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for BlurEffectPlugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let sigma_x = params
            .get("sigma_x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let sigma_y = params
            .get("sigma_y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let tile_mode_str = params
            .get("tile_mode")
            .and_then(|pv| pv.get_as::<String>())
            .unwrap_or_else(|| "clamp".to_string());

        let tile_mode = match tile_mode_str.as_str() {
            "clamp" => TileMode::Clamp,
            "repeat" => TileMode::Repeat,
            "mirror" => TileMode::Mirror,
            "decal" => TileMode::Decal,
            _ => TileMode::Clamp,
        };

        if sigma_x <= 0.0 && sigma_y <= 0.0 {
            return Ok(input.clone());
        }

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, _width, _height| {
            image_filters::blur(
                (sigma_x as f32, sigma_y as f32),
                Some(tile_mode),
                None,
                None,
            )
            .ok_or(LibraryError::render(
                "Failed to create blur filter".to_string(),
            ))
        })
    }

    fn properties(&self) -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::PropertyValue;
        use crate::model::project::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "sigma_x",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: false,
                },
                "Sigma X",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "sigma_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: false,
                },
                "Sigma Y",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "tile_mode",
                PropertyUiType::Dropdown {
                    options: vec![
                        "clamp".to_string(),
                        "repeat".to_string(),
                        "mirror".to_string(),
                        "decal".to_string(),
                    ],
                },
                "Tile Mode",
                PropertyValue::String("clamp".to_string()),
            ),
        ]
    }
}
