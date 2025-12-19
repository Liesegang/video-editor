use crate::error::LibraryError;
use crate::core::model::property::PropertyValue;
use crate::extensions::traits::{EffectPlugin, Plugin};
use crate::graphics::renderer::RenderOutput;
use crate::graphics::skia_utils::GpuContext;
use skia_safe::image_filters;
use std::collections::HashMap;

pub struct DilateEffectPlugin;

impl DilateEffectPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for DilateEffectPlugin {
    fn id(&self) -> &'static str {
        "dilate"
    }

    fn name(&self) -> String {
        "Dilate".to_string()
    }

    fn category(&self) -> String {
        "Morphology".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for DilateEffectPlugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let radius_x = params
            .get("radius_x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let radius_y = params
            .get("radius_y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);

        if radius_x <= 0.0 && radius_y <= 0.0 {
            return Ok(input.clone());
        }

        use crate::compositing::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, _width, _height| {
            image_filters::dilate((radius_x as f32, radius_y as f32), None, None).ok_or(
                LibraryError::Render("Failed to create dilate filter".to_string()),
            )
        })
    }

    fn properties(&self) -> Vec<crate::extensions::traits::PropertyDefinition> {
        use crate::extensions::traits::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition {
                name: "radius_x".to_string(),
                label: "Radius X".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Dilate".to_string(),
            },
            PropertyDefinition {
                name: "radius_y".to_string(),
                label: "Radius Y".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Dilate".to_string(),
            },
        ]
    }
}
