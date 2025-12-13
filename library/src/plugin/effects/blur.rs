use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin, PluginCategory};
use crate::rendering::renderer::{RenderOutput, TextureInfo};
use crate::rendering::skia_utils::{
    GpuContext, image_to_skia, surface_to_image,
};
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
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let sigma_x = params
            .get("sigma_x")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let sigma_y = params
            .get("sigma_y")
            .and_then(|pv| pv.get_as::<f64>())
            .unwrap_or(0.0);
        let tile_mode_str = params
            .get("tile_mode")
            .and_then(|pv| pv.get_as::<String>())
            .unwrap_or_else(|| "clamp".to_string());

        let tile_mode = match tile_mode_str.as_str() {
            "clamp" => TileMode::Clamp,
            "repeat" => TileMode::Repeat,
            "mirror" => TileMode::Mirror,
            "decal" => TileMode::Decal,
            _ => TileMode::Clamp,
        };

        if sigma_x <= 0.0 && sigma_y <= 0.0 {
            return Ok(input.clone());
        }

        let perform_blur = |image: &skia_safe::Image,
                            width: u32,
                            height: u32,
                            context: Option<&mut skia_safe::gpu::DirectContext>|
         -> Result<RenderOutput, LibraryError> {
            let mut surface = crate::rendering::skia_utils::create_surface(width, height, context)?;
            let canvas = surface.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);

            let mut paint = Paint::default();
            let filter =
                image_filters::blur((sigma_x as f32, sigma_y as f32), Some(tile_mode), None, None)
                    .ok_or(LibraryError::Render(
                    "Failed to create blur filter".to_string(),
                ))?;
            paint.set_image_filter(filter);
            canvas.draw_image(image, (0, 0), Some(&paint));

            // If we have a context, try to return a texture
            let ctx_opt = surface.recording_context();
            if let Some(mut ctx) = ctx_opt {
                if let Some(mut dctx) = ctx.as_direct_context() {
                    dctx.flush_and_submit();
                }
                
                if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                    &mut surface,
                    skia_safe::surface::BackendHandleAccess::FlushRead,
                ) {
                    if let Some(gl_info) = texture.gl_texture_info() {
                        return Ok(RenderOutput::Texture(TextureInfo {
                            texture_id: gl_info.id,
                            width,
                            height,
                        }));
                    }
                }
            }
            // Fallback to Image
            let image = surface_to_image(&mut surface, width, height)?;
            Ok(RenderOutput::Image(image))
        };

        match input {
            RenderOutput::Texture(info) => {
                if let Some(ctx) = gpu_context {
                    let image = crate::rendering::skia_utils::create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?;
                    perform_blur(
                        &image,
                        info.width,
                        info.height,
                        Some(&mut ctx.direct_context),
                    )
                } else {
                    Err(LibraryError::Render(
                        "Texture input without GPU context".to_string(),
                    ))
                }
            }
            RenderOutput::Image(img) => {
                let sk_image = image_to_skia(img)?;
                if let Some(ctx) = gpu_context {
                    perform_blur(
                        &sk_image,
                        img.width,
                        img.height,
                        Some(&mut ctx.direct_context),
                    )
                } else {
                    perform_blur(&sk_image, img.width, img.height, None)
                }
            }
        }
    }

    fn properties(&self) -> Vec<crate::plugin::PropertyDefinition> {
        use crate::plugin::{PropertyDefinition, PropertyUiType};
        use crate::model::project::property::PropertyValue;
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition {
                name: "sigma_x".to_string(),
                label: "Sigma X".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Blur".to_string(),
            },
            PropertyDefinition {
                name: "sigma_y".to_string(),
                label: "Sigma Y".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Blur".to_string(),
            },
            PropertyDefinition {
                name: "tile_mode".to_string(),
                label: "Tile Mode".to_string(),
                ui_type: PropertyUiType::Dropdown {
                    options: vec![
                        "clamp".to_string(),
                        "repeat".to_string(),
                        "mirror".to_string(),
                        "decal".to_string(),
                    ],
                },
                default_value: PropertyValue::String("clamp".to_string()),
                category: "Blur".to_string(),
            },
        ]
    }
}
