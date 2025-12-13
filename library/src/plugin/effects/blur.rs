use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin, PluginCategory};
use crate::rendering::renderer::{RenderOutput, TextureInfo};
use crate::rendering::skia_utils::{
    GpuContext, image_to_skia, surface_to_image,
};
use skia_safe::{Paint, TileMode, image_filters};
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

    fn category(&self) -> PluginCategory {
        PluginCategory::Effect
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
            image_filters::blur((sigma_x as f32, sigma_y as f32), Some(tile_mode), None, None)
                .ok_or(LibraryError::Render(
                    "Failed to create blur filter".to_string(),
                ))
        })
    }

    fn properties(&self) -> Vec<crate::plugin::PropertyDefinition> {
        use crate::plugin::{PropertyDefinition, PropertyUiType};
        use crate::model::project::property::PropertyValue;
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition {
                name: "sigma_x".to_string(),
                label: "Sigma X".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Blur".to_string(),
            },
            PropertyDefinition {
                name: "sigma_y".to_string(),
                label: "Sigma Y".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Blur".to_string(),
            },
            PropertyDefinition {
                name: "tile_mode".to_string(),
                label: "Tile Mode".to_string(),
                ui_type: PropertyUiType::Dropdown {
                    options: vec![
                        "clamp".to_string(),
                        "repeat".to_string(),
                        "mirror".to_string(),
                        "decal".to_string(),
                    ],
                },
                default_value: PropertyValue::String("clamp".to_string()),
                category: "Blur".to_string(),
            },
        ]
    }
}
