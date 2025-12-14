use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{Rect, image_filters};
use std::collections::HashMap;

pub struct TileEffectPlugin;

impl TileEffectPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for TileEffectPlugin {
    fn id(&self) -> &'static str {
        "tile"
    }

    fn name(&self) -> String {
        "Tile".to_string()
    }

    fn category(&self) -> String {
        "Distortion".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for TileEffectPlugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let x = params
            .get("x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let y = params
            .get("y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let width = params
            .get("width")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let height = params
            .get("height") // Fixed typo: was "width" in mag
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);

        if width <= 0.0 || height <= 0.0 {
            return Ok(input.clone());
        }

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, canvas_width, canvas_height| {
            let src_rect = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32);
            // Destination is the full canvas
            let dst_rect = Rect::from_wh(canvas_width as f32, canvas_height as f32);

            image_filters::tile(src_rect, dst_rect, None).ok_or(LibraryError::Render(
                "Failed to create tile filter".to_string(),
            ))
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
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Tile".to_string(),
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
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Tile".to_string(),
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
                category: "Tile".to_string(),
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
                category: "Tile".to_string(),
            },
        ]
    }
}
