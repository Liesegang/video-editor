use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin, PluginCategory};
use crate::rendering::skia_utils::{create_raster_surface, image_to_skia, surface_to_image};
use skia_safe::{Paint, TileMode, image_filters};
use std::collections::HashMap;

pub struct BlurEffectPlugin;

impl BlurEffectPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for BlurEffectPlugin {
    fn id(&self) -> &'static str {
        "blur"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Effect
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for BlurEffectPlugin {
    fn apply(
        &self,
        image: &Image,
        params: &HashMap<String, PropertyValue>,
    ) -> Result<Image, LibraryError> {
        let radius = params
            .get("blur_radius")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        if radius <= 0.0 {
            return Ok(image.clone());
        }

        let sk_image = image_to_skia(image)?;
        let mut surface = create_raster_surface(image.width, image.height)?;
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

        let mut paint = Paint::default();
        let filter =
            image_filters::blur((radius as f32, radius as f32), None::<TileMode>, None, None)
                .ok_or(LibraryError::Render(
                    "Failed to create blur filter".to_string(),
                ))?;
        paint.set_image_filter(filter);
        canvas.draw_image(&sk_image, (0, 0), Some(&paint));

        surface_to_image(&mut surface, image.width, image.height)
    }
}
