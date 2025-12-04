use crate::loader::image::Image;
use crate::model::frame::effect::ImageEffect;
use crate::model::project::property::PropertyValue;
use crate::rendering::skia_utils::{create_raster_surface, image_to_skia, surface_to_image};
use log::warn;
use skia_safe::{Paint, TileMode, image_filters};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

type EffectFn =
    dyn Fn(&Image, &HashMap<String, PropertyValue>) -> Result<Image, Box<dyn Error>> + Send + Sync;

pub struct EffectRegistry {
    handlers: HashMap<String, Arc<EffectFn>>,
}

impl EffectRegistry {
    pub fn new_with_defaults() -> Self {
        let mut registry = Self {
            handlers: HashMap::new(),
        };
        registry.register_effect("blur", Arc::new(blur_effect));
        registry
    }

    pub fn register_effect(&mut self, name: &str, handler: Arc<EffectFn>) {
        self.handlers.insert(name.to_string(), handler);
    }

    pub fn apply(
        &self,
        mut image: Image,
        effects: &[ImageEffect],
    ) -> Result<Image, Box<dyn Error>> {
        for effect in effects {
            if let Some(handler) = self.handlers.get(effect.effect_type.as_str()) {
                image = handler(&image, &effect.properties)?;
            } else {
                warn!(
                    "Image effect '{}' is not registered; skipping",
                    effect.effect_type
                );
            }
        }
        Ok(image)
    }
}

fn blur_effect(
    image: &Image,
    props: &HashMap<String, PropertyValue>,
) -> Result<Image, Box<dyn Error>> {
    let radius = props
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
    let filter = image_filters::blur((radius as f32, radius as f32), None::<TileMode>, None, None)
        .ok_or("Failed to create blur filter")?;
    paint.set_image_filter(filter);
    canvas.draw_image(&sk_image, (0, 0), Some(&paint));

    surface_to_image(&mut surface, image.width, image.height)
}
