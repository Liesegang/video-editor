use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin, PluginCategory};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{image_filters, Color};
use std::collections::HashMap;

pub struct DropShadowEffectPlugin;

impl DropShadowEffectPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for DropShadowEffectPlugin {
    fn id(&self) -> &'static str {
        "drop_shadow"
    }

    fn name(&self) -> String {
        "Drop Shadow".to_string()
    }

    fn category(&self) -> String {
        "Perspective".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

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
            .and_then(|pv| pv.get_as::<crate::model::frame::color::Color>())
            .unwrap_or(crate::model::frame::color::Color { r: 0, g: 0, b: 0, a: 255 });
        let shadow_only = params
            .get("shadow_only")
            .and_then(|pv| pv.get_as::<bool>())
            .unwrap_or(false);

        if dx == 0.0 && dy == 0.0 && sigma_x == 0.0 && sigma_y == 0.0 {
            if !shadow_only {
                 return Ok(input.clone());
            }
        }

        use crate::plugin::effects::utils::apply_skia_filter;
        
        // Convert internal Color to Skia Color
        let skia_color = Color::from_argb(color_val.a, color_val.r, color_val.g, color_val.b);

        apply_skia_filter(input, gpu_context, |_image, _width, _height| {
            if shadow_only {
                 image_filters::drop_shadow_only((dx as f32, dy as f32), (sigma_x as f32, sigma_y as f32), skia_color, None, None, None)
                    .ok_or(LibraryError::Render(
                        "Failed to create drop shadow only filter".to_string(),
                    ))
            } else {
                image_filters::drop_shadow((dx as f32, dy as f32), (sigma_x as f32, sigma_y as f32), skia_color, None, None, None)
                    .ok_or(LibraryError::Render(
                        "Failed to create drop shadow filter".to_string(),
                    ))
            }
        })
    }

    fn properties(&self) -> Vec<crate::plugin::PropertyDefinition> {
        use crate::plugin::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition {
                name: "dx".to_string(),
                label: "Distance X".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -500.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(5.0)),
                category: "Drop Shadow".to_string(),
            },
            PropertyDefinition {
                name: "dy".to_string(),
                label: "Distance Y".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -500.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(5.0)),
                category: "Drop Shadow".to_string(),
            },
            PropertyDefinition {
                name: "sigma_x".to_string(),
                label: "Blur X".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(3.0)),
                category: "Drop Shadow".to_string(),
            },
             PropertyDefinition {
                name: "sigma_y".to_string(),
                label: "Blur Y".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(3.0)),
                category: "Drop Shadow".to_string(),
            },
            PropertyDefinition {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui_type: PropertyUiType::Color,
                default_value: PropertyValue::Color(crate::model::frame::color::Color { r: 0, g: 0, b: 0, a: 255 }),
                category: "Drop Shadow".to_string(),
            },
            PropertyDefinition {
                name: "shadow_only".to_string(),
                label: "Shadow Only".to_string(),
                ui_type: PropertyUiType::Bool,
                default_value: PropertyValue::Boolean(false),
                category: "Drop Shadow".to_string(),
            },
        ]
    }
}
