use crate::error::LibraryError;
use crate::model::property::PropertyValue;
use crate::plugin::{EffectColorDomain, EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{Rect, image_filters};
use std::collections::HashMap;

#[derive(Default)]
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

    fn name(&self) -> String {
        "Magnifier".to_string()
    }

    fn category(&self) -> String {
        "Distortion".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for MagnifierEffectPlugin {
    fn color_domain(&self) -> EffectColorDomain {
        EffectColorDomain::ProjectLinearPreserving
    }

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
        let lens_width = params
            .get("width")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let lens_height = params
            .get("height")
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

        if lens_width <= 0.0 || lens_height <= 0.0 {
            return Ok(input.clone());
        }

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(
            input,
            gpu_context,
            |_image, _canvas_width, _canvas_height| {
                let lens_bounds =
                    Rect::from_xywh(x as f32, y as f32, lens_width as f32, lens_height as f32);
                image_filters::magnifier(
                    lens_bounds,
                    zoom_amount as f32,
                    inset as f32,
                    skia_safe::SamplingOptions::default(),
                    None, // input
                    None, // crop
                )
                .ok_or(LibraryError::Render(
                    "Failed to create magnifier filter".to_string(),
                ))
            },
        )
    }

    fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType};
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

#[cfg(test)]
mod tests {
    use super::MagnifierEffectPlugin;
    use crate::model::frame::Image;
    use crate::model::property::PropertyValue;
    use crate::plugin::EffectPlugin;
    use crate::rendering::renderer::RenderOutput;
    use ordered_float::OrderedFloat;
    use std::collections::HashMap;

    #[test]
    fn zero_height_bypasses_even_when_width_is_positive() {
        let input = RenderOutput::Image(Image::new(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        ));
        let params = HashMap::from([
            (
                "width".to_string(),
                PropertyValue::Number(OrderedFloat(20.0)),
            ),
            (
                "height".to_string(),
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
        ]);

        let output = MagnifierEffectPlugin
            .apply(&input, &params, None)
            .expect("zero-height magnifier should bypass");
        let output_data = match &output {
            RenderOutput::Image(image) => Some(image.data.as_slice()),
            RenderOutput::Working(_) | RenderOutput::Texture(_) => None,
        };
        let input_data = match &input {
            RenderOutput::Image(image) => Some(image.data.as_slice()),
            RenderOutput::Working(_) | RenderOutput::Texture(_) => None,
        };
        assert_eq!(output_data, input_data);
    }
}
