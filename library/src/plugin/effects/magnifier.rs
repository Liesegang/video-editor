use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin, PluginCategory};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{image_filters, Rect};
use std::collections::HashMap;

pub struct MagnifierEffectPlugin;

impl MagnifierEffectPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for MagnifierEffectPlugin {
    fn id(&self) -> &'static str {
        "magnifier"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Effect
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for MagnifierEffectPlugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let x = params
            .get("x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let y = params
            .get("y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let width = params
            .get("width")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let height = params
            .get("width")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let zoom_amount = params
            .get("zoom_amount")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(2.0);
        let inset = params
            .get("inset")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);

         if width <= 0.0 || height <= 0.0 {
            return Ok(input.clone());
        }

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, width, height| {
            let lens_bounds = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32);
            image_filters::magnifier(
                lens_bounds,
                zoom_amount as f32,
                inset as f32,
                skia_safe::SamplingOptions::default(),
                None, // input
                None, // crop
            ).ok_or(LibraryError::Render("Failed to create magnifier filter".to_string()))
        })
    }

    fn properties(&self) -> Vec<crate::plugin::PropertyDefinition> {
        use crate::plugin::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition {
                name: "x".to_string(),
                label: "X".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -10000.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(100.0)),
                category: "Magnifier".to_string(),
            },
            PropertyDefinition {
                name: "y".to_string(),
                label: "Y".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -10000.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(100.0)),
                category: "Magnifier".to_string(),
            },
            PropertyDefinition {
                name: "width".to_string(),
                label: "Width".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(100.0)),
                category: "Magnifier".to_string(),
            },
             PropertyDefinition {
                name: "height".to_string(),
                label: "Height".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(100.0)),
                category: "Magnifier".to_string(),
            },
            PropertyDefinition {
                name: "zoom_amount".to_string(),
                label: "Zoom Amount".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 1.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "x".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(2.0)),
                category: "Magnifier".to_string(),
            },
            PropertyDefinition {
                name: "inset".to_string(),
                label: "Inset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Magnifier".to_string(),
            },
        ]
    }
}
