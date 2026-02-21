use crate::error::LibraryError;
use crate::plugin::EffectPlugin;
use crate::project::property::PropertyValue;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{Rect, image_filters};
use std::collections::HashMap;

super::define_effect_plugin!(
    MagnifierEffectPlugin,
    id: "magnifier",
    name: "Magnifier",
    category: "Distortion",
    version: (0, 1, 0)
);

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

        use crate::builtin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, width, height| {
            let lens_bounds = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32);
            image_filters::magnifier(
                lens_bounds,
                zoom_amount as f32,
                inset as f32,
                skia_safe::SamplingOptions::default(),
                None, // input
                None, // crop
            )
            .ok_or(LibraryError::render(
                "Failed to create magnifier filter".to_string(),
            ))
        })
    }

    fn properties(&self) -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "x",
                PropertyUiType::Float {
                    min: -10000.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "X",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
            PropertyDefinition::new(
                "y",
                PropertyUiType::Float {
                    min: -10000.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Y",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
            PropertyDefinition::new(
                "width",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Width",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
            PropertyDefinition::new(
                "height",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Height",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
            PropertyDefinition::new(
                "zoom_amount",
                PropertyUiType::Float {
                    min: 1.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "x".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Zoom Amount",
                PropertyValue::Number(OrderedFloat(2.0)),
            ),
            PropertyDefinition::new(
                "inset",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Inset",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
        ]
    }
}
