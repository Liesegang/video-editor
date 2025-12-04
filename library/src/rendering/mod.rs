pub mod effects;
pub mod queue;
pub mod renderer;
pub mod skia_renderer;
pub mod skia_utils;

use crate::framing::PropertyEvaluatorRegistry;
use crate::loader::image::Image;
use crate::model::frame::entity::{FrameEntity, FrameObject};
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
use crate::plugin::{LoadRequest, LoadResponse, PluginManager};
use crate::rendering::effects::EffectRegistry;
use crate::rendering::renderer::Renderer;
use crate::util::timing::{ScopedTimer, measure_debug};
use std::error::Error;
use std::sync::Arc;

pub struct RenderContext<T: Renderer> {
    pub renderer: T,
    plugin_manager: Arc<PluginManager>,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    effect_registry: Arc<EffectRegistry>,
}

impl<T: Renderer> RenderContext<T> {
    pub fn new(
        renderer: T,
        plugin_manager: Arc<PluginManager>,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        effect_registry: Arc<EffectRegistry>,
    ) -> Self {
        RenderContext {
            renderer,
            plugin_manager,
            property_evaluators,
            effect_registry,
        }
    }

    pub fn render_frame(&mut self, frame_info: FrameInfo) -> Result<Image, Box<dyn Error>> {
        let object_count = frame_info.objects.len();
        let _timer = ScopedTimer::debug(format!(
            "RenderContext::render_frame objects={}",
            object_count
        ));

        for frame_object in frame_info.objects {
            let FrameObject {
                entity,
                properties: _properties,
            } = frame_object;
            match entity {
                FrameEntity::Video {
                    surface,
                    frame_number,
                } => {
                    let request = LoadRequest::VideoFrame {
                        path: surface.file_path.clone(),
                        frame_number,
                    };
                    let video_frame = measure_debug(
                        format!("Decode video {} frame {}", surface.file_path, frame_number),
                        || -> Result<Image, Box<dyn Error>> {
                            match self.plugin_manager.load_resource(&request)? {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    let final_image = self.apply_effects(video_frame, &surface.effects)?;
                    measure_debug(format!("Draw video {}", surface.file_path), || {
                        self.renderer.draw_image(&final_image, &surface.transform)
                    })?;
                }
                FrameEntity::Image { surface } => {
                    let request = LoadRequest::Image {
                        path: surface.file_path.clone(),
                    };
                    let image_frame = measure_debug(
                        format!("Load image {}", surface.file_path),
                        || -> Result<Image, Box<dyn Error>> {
                            match self.plugin_manager.load_resource(&request)? {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    let final_image = self.apply_effects(image_frame, &surface.effects)?;
                    measure_debug(format!("Draw image {}", surface.file_path), || {
                        self.renderer.draw_image(&final_image, &surface.transform)
                    })?;
                }
                FrameEntity::Text {
                    text,
                    font,
                    size,
                    color,
                    effects,
                    transform,
                } => {
                    let text_layer =
                        measure_debug(format!("Rasterize text layer '{}'", text), || {
                            self.renderer
                                .rasterize_text_layer(&text, size, &font, &color, &transform)
                        })?;
                    let final_image = self.apply_effects(text_layer, &effects)?;
                    measure_debug(format!("Composite text '{}'", text), || {
                        self.renderer
                            .draw_image(&final_image, &Transform::default())
                    })?;
                }
                FrameEntity::Shape {
                    path,
                    styles,
                    path_effects,
                    effects,
                    transform,
                } => {
                    let shape_layer =
                        measure_debug(format!("Rasterize shape layer {}", path), || {
                            self.renderer.rasterize_shape_layer(
                                &path,
                                &styles,
                                &path_effects,
                                &transform,
                            )
                        })?;
                    let final_image = self.apply_effects(shape_layer, &effects)?;
                    measure_debug(format!("Composite shape {}", path), || {
                        self.renderer
                            .draw_image(&final_image, &Transform::default())
                    })?;
                }
            }
        }
        measure_debug("RenderContext::finalize", || self.renderer.finalize())
    }

    pub fn clear(&mut self) -> Result<(), Box<dyn Error>> {
        measure_debug("RenderContext::clear", || self.renderer.clear())
    }

    pub fn property_evaluators(&self) -> Arc<PropertyEvaluatorRegistry> {
        Arc::clone(&self.property_evaluators)
    }

    fn apply_effects(
        &self,
        image: Image,
        effects: &[crate::model::frame::effect::ImageEffect],
    ) -> Result<Image, Box<dyn Error>> {
        if effects.is_empty() {
            Ok(image)
        } else {
            self.effect_registry.apply(image, effects)
        }
    }
}
