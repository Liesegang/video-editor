use crate::error::LibraryError;
use crate::model::property::PropertyValue;
use crate::plugin::{EffectColorDomain, EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use skia_safe::{Rect, image_filters};
use std::collections::HashMap;

#[derive(Default)]
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
        (0, 2, 0)
    }
}

impl EffectPlugin for TileEffectPlugin {
    fn color_domain(&self) -> EffectColorDomain {
        EffectColorDomain::ProjectLinearPreserving
    }

    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let offset_x = params
            .get("offset_x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let offset_y = params
            .get("offset_y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let width = params
            .get("width")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);
        let height = params
            .get("height")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(100.0);

        if width <= 0.0 || height <= 0.0 {
            return Ok(input.clone());
        }

        use crate::plugin::effects::utils::apply_skia_filter;

        apply_skia_filter(input, gpu_context, |_image, canvas_width, canvas_height| {
            let src_rect = centered_source_rect(
                canvas_width,
                canvas_height,
                width,
                height,
                offset_x,
                offset_y,
            );
            // Destination is the full canvas
            let dst_rect = Rect::from_wh(canvas_width as f32, canvas_height as f32);

            image_filters::tile(src_rect, dst_rect, None).ok_or(LibraryError::Render(
                "Failed to create tile filter".to_string(),
            ))
        })
    }

    fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "offset_x",
                PropertyUiType::Float {
                    min: -10000.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Offset X",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "offset_y",
                PropertyUiType::Float {
                    min: -10000.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Offset Y",
                PropertyValue::Number(OrderedFloat(0.0)),
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
        ]
    }
}

/// Builds Tile's source rectangle in canvas-local coordinates.
///
/// Authored offsets are relative to the canvas centre: `(0, 0)` keeps the
/// source tile centred, regardless of the composition resolution or tile
/// dimensions. This is also the coordinate convention used by Preview gizmos
/// and avoids resolution-dependent "magic" top-left values in Inspector.
fn centered_source_rect(
    canvas_width: u32,
    canvas_height: u32,
    tile_width: f64,
    tile_height: f64,
    offset_x: f64,
    offset_y: f64,
) -> Rect {
    let tile_width = tile_width as f32;
    let tile_height = tile_height as f32;
    let center_x = canvas_width as f32 * 0.5 + offset_x as f32;
    let center_y = canvas_height as f32 * 0.5 + offset_y as f32;
    Rect::from_xywh(
        center_x - tile_width * 0.5,
        center_y - tile_height * 0.5,
        tile_width,
        tile_height,
    )
}

#[cfg(test)]
mod tests {
    use super::{TileEffectPlugin, centered_source_rect};
    use crate::model::frame::Image;
    use crate::model::property::PropertyValue;
    use crate::plugin::{EffectPlugin, Plugin};
    use crate::rendering::renderer::RenderOutput;
    use std::collections::HashMap;

    #[test]
    fn zero_offset_places_the_tile_around_the_canvas_centre() {
        let rect = centered_source_rect(640, 360, 100.0, 80.0, 0.0, 0.0);
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (270.0, 140.0, 370.0, 220.0)
        );
    }

    #[test]
    fn offsets_are_relative_to_the_canvas_centre() {
        let rect = centered_source_rect(640, 360, 100.0, 80.0, -20.0, 35.0);
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (250.0, 175.0, 350.0, 255.0)
        );
    }

    #[test]
    fn published_properties_use_explicit_offset_names() {
        let plugin = TileEffectPlugin::new();
        let properties = plugin.properties();
        let keys = properties
            .iter()
            .map(|property| property.name())
            .collect::<Vec<_>>();
        assert_eq!(keys, ["offset_x", "offset_y", "width", "height"]);
        assert_eq!(plugin.version(), (0, 2, 0));
        assert_eq!(
            properties[0].default_value(),
            &PropertyValue::from(0.0),
            "zero offset must be the centred default"
        );
    }

    #[test]
    fn zero_offset_keeps_the_central_source_tile_in_place() {
        let mut pixels = Vec::new();
        for index in 0_u8..16 {
            pixels.extend_from_slice(&[index, 0, 0, 255]);
        }
        let input = RenderOutput::Image(Image::new(4, 4, pixels));
        let params = HashMap::from([
            ("width".to_string(), PropertyValue::from(2.0)),
            ("height".to_string(), PropertyValue::from(2.0)),
        ]);

        let output = TileEffectPlugin::new()
            .apply(&input, &params, None)
            .expect("centred Tile render");
        let RenderOutput::Image(output) = output else {
            panic!("CPU Tile must return an owned image");
        };
        let RenderOutput::Image(input) = input else {
            panic!("the Tile test input must remain an owned image");
        };
        for y in 1_usize..3 {
            for x in 1_usize..3 {
                let start = (y * 4 + x) * 4;
                assert_eq!(
                    &output.data[start..start + 4],
                    &input.data[start..start + 4],
                    "the central source tile moved at ({x}, {y})"
                );
            }
        }
    }
}
