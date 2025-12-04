pub mod queue;
pub mod renderer;
pub mod skia_renderer;

use crate::framing::PropertyEvaluatorRegistry;
use crate::loader::image::Image;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::{FrameEntity, FrameObject};
use crate::model::frame::frame::FrameInfo;
use crate::plugin::{LoadRequest, LoadResponse, PluginManager};
use crate::rendering::renderer::Renderer;
use crate::util::timing::{ScopedTimer, measure_debug};
use std::error::Error;
use std::sync::Arc;

pub struct RenderContext<T: Renderer> {
    pub renderer: T,
    plugin_manager: Arc<PluginManager>,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
}

impl<T: Renderer> RenderContext<T> {
    pub fn new(
        renderer: T,
        plugin_manager: Arc<PluginManager>,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
    ) -> Self {
        RenderContext {
            renderer,
            plugin_manager,
            property_evaluators,
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
                    file_path,
                    frame_number,
                    transform,
                } => {
                    let request = LoadRequest::VideoFrame {
                        path: file_path.clone(),
                        frame_number,
                    };
                    let video_frame = measure_debug(
                        format!("Decode video {} frame {}", file_path, frame_number),
                        || -> Result<Image, Box<dyn Error>> {
                            match self.plugin_manager.load_resource(&request)? {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    measure_debug(format!("Draw video {}", file_path), || {
                        self.renderer.draw_image(&video_frame, &transform)
                    })?;
                }
                FrameEntity::Image {
                    file_path,
                    transform,
                } => {
                    let request = LoadRequest::Image {
                        path: file_path.clone(),
                    };
                    let image_frame = measure_debug(
                        format!("Load image {}", file_path),
                        || -> Result<Image, Box<dyn Error>> {
                            match self.plugin_manager.load_resource(&request)? {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    measure_debug(format!("Draw image {}", file_path), || {
                        self.renderer.draw_image(&image_frame, &transform)
                    })?;
                }
                FrameEntity::Text {
                    text,
                    font,
                    size,
                    color,
                    transform,
                } => {
                    measure_debug(format!("Draw text '{}'", text), || {
                        self.renderer
                            .draw_text(&text, size, &font, &color, &transform)
                    })?;
                }
                FrameEntity::Shape {
                    path,
                    styles,
                    path_effects,
                    transform,
                } => {
                    let style_count = styles.len();
                    let effect_count = path_effects.len();
                    let _shape_timer = ScopedTimer::debug(format!(
                        "Draw shape path={} styles={} effects={}",
                        path, style_count, effect_count
                    ));
                    for style in styles {
                        match style {
                            DrawStyle::Fill { color } => {
                                measure_debug(format!("Shape fill path={}", path), || {
                                    self.renderer.draw_shape_fill(
                                        &path,
                                        &color,
                                        &path_effects,
                                        &transform,
                                    )
                                })?;
                            }
                            DrawStyle::Stroke {
                                color,
                                width,
                                cap,
                                join,
                                miter,
                            } => {
                                measure_debug(
                                    format!("Shape stroke path={} width={}", path, width),
                                    || {
                                        self.renderer.draw_shape_stroke(
                                            &path,
                                            &color,
                                            &path_effects,
                                            width,
                                            cap,
                                            join,
                                            miter,
                                            &transform,
                                        )
                                    },
                                )?;
                            }
                        }
                    }
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
}
