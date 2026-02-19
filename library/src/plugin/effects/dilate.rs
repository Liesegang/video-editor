use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
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

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, _width, _height| {
            image_filters::dilate((radius_x as f32, radius_y as f32), None, None).ok_or(
                LibraryError::render("Failed to create dilate filter".to_string()),
            )
        })
    }

    fn properties(&self) -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "radius_x",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Radius X",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "radius_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Radius Y",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
        ]
    }
}
