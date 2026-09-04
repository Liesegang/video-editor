use crate::error::LibraryError;
use crate::model::property::PropertyValue;
use crate::plugin::{EffectColorDomain, EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{TileMode, image_filters};
use std::collections::HashMap;

#[derive(Default)]
pub struct BlurEffectPlugin;

// Skia's direct full-resolution blur becomes disproportionately expensive on
// the CPU fallback used by Preview. Keep the interactive primitive bounded;
// larger-radius looks should be built from a downsampled Module pipeline.
const MAX_INTERACTIVE_SIGMA: f64 = 32.0;

fn interactive_sigma(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_INTERACTIVE_SIGMA) as f32
    } else {
        0.0
    }
}

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
    fn color_domain(&self) -> EffectColorDomain {
        EffectColorDomain::ProjectLinearPreserving
    }

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

        let sigma_x = interactive_sigma(sigma_x);
        let sigma_y = interactive_sigma(sigma_y);

        if sigma_x <= 0.0 && sigma_y <= 0.0 {
            return Ok(input.clone());
        }

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, _width, _height| {
            image_filters::blur((sigma_x, sigma_y), Some(tile_mode), None, None).ok_or(
                LibraryError::Render("Failed to create blur filter".to_string()),
            )
        })
    }

    fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::PropertyValue;
        use crate::model::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "sigma_x",
                PropertyUiType::Float {
                    min: 0.0,
                    max: MAX_INTERACTIVE_SIGMA,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Sigma X",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "sigma_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: MAX_INTERACTIVE_SIGMA,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::PropertyUiType;
    use crate::plugin::EffectPlugin;

    #[test]
    fn full_resolution_blur_is_bounded_for_interactive_preview() {
        assert_eq!(interactive_sigma(60.0), MAX_INTERACTIVE_SIGMA as f32);
        assert_eq!(interactive_sigma(f64::NAN), 0.0);

        for definition in BlurEffectPlugin::new().properties() {
            if !matches!(definition.name(), "sigma_x" | "sigma_y") {
                continue;
            }
            let PropertyUiType::Float {
                max,
                max_hard_limit,
                ..
            } = definition.ui_type()
            else {
                panic!("blur sigma must be numeric");
            };
            assert_eq!(*max, MAX_INTERACTIVE_SIGMA);
            assert!(*max_hard_limit);
        }
    }
}
