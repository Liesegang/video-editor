use crate::error::LibraryError;
use crate::plugin::EffectPlugin;
use crate::project::property::PropertyValue;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{Color, image_filters};
use std::collections::HashMap;

super::define_effect_plugin!(
    DropShadowEffectPlugin,
    id: "drop_shadow",
    name: "Drop Shadow",
    category: "Perspective",
    version: (0, 1, 0)
);

impl EffectPlugin for DropShadowEffectPlugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let dx = params
            .get("dx")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let dy = params
            .get("dy")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let sigma_x = params
            .get("sigma_x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let sigma_y = params
            .get("sigma_y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let color_val = params
            .get("color")
            .and_then(|pv| pv.get_as::<crate::runtime::color::Color>())
            .unwrap_or(crate::runtime::color::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            });
        let shadow_only = params
            .get("shadow_only")
            .and_then(|pv| pv.get_as::<bool>())
            .unwrap_or(false);

        if dx == 0.0 && dy == 0.0 && sigma_x == 0.0 && sigma_y == 0.0 {
            if !shadow_only {
                return Ok(input.clone());
            }
        }

        use crate::builtin::effects::utils::apply_skia_filter;

        // Convert internal Color to Skia Color
        let skia_color = Color::from_argb(color_val.a, color_val.r, color_val.g, color_val.b);

        apply_skia_filter(input, gpu_context, |_image, _width, _height| {
            if shadow_only {
                image_filters::drop_shadow_only(
                    (dx as f32, dy as f32),
                    (sigma_x as f32, sigma_y as f32),
                    skia_color,
                    None,
                    None,
                    None,
                )
                .ok_or(LibraryError::render(
                    "Failed to create drop shadow only filter".to_string(),
                ))
            } else {
                image_filters::drop_shadow(
                    (dx as f32, dy as f32),
                    (sigma_x as f32, sigma_y as f32),
                    skia_color,
                    None,
                    None,
                    None,
                )
                .ok_or(LibraryError::render(
                    "Failed to create drop shadow filter".to_string(),
                ))
            }
        })
    }

    fn properties(&self) -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "dx",
                PropertyUiType::Float {
                    min: -500.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Distance X",
                PropertyValue::Number(OrderedFloat(5.0)),
            ),
            PropertyDefinition::new(
                "dy",
                PropertyUiType::Float {
                    min: -500.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Distance Y",
                PropertyValue::Number(OrderedFloat(5.0)),
            ),
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
                "Blur X",
                PropertyValue::Number(OrderedFloat(3.0)),
            ),
            PropertyDefinition::new(
                "sigma_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Blur Y",
                PropertyValue::Number(OrderedFloat(3.0)),
            ),
            PropertyDefinition::new(
                "color",
                PropertyUiType::Color,
                "Color",
                PropertyValue::Color(crate::runtime::color::Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
            ),
            PropertyDefinition::new(
                "shadow_only",
                PropertyUiType::Bool,
                "Shadow Only",
                PropertyValue::Boolean(false),
            ),
        ]
    }
}
