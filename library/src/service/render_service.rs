use crate::cache::SharedCacheManager;
use crate::error::LibraryError;
use crate::framing::entity_converters::EntityConverterRegistry;
use crate::framing::get_frame_from_project;
use crate::loader::image::Image;
use crate::model::frame::entity::{FrameContent, FrameObject}; // FrameEntity -> FrameContent
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
use crate::plugin::{LoadRequest, LoadResponse, PluginManager};
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::service::project_model::ProjectModel;
use crate::util::timing::{measure_debug, measure_debug_lazy, ScopedTimer};
// Removed HashMap and EvaluationContext imports
use std::sync::Arc;

pub struct RenderService<T: Renderer> {
    pub renderer: T,
    cache_manager: SharedCacheManager,
    plugin_manager: Arc<PluginManager>,
    entity_converter_registry: Arc<EntityConverterRegistry>,
}

impl<T: Renderer> RenderService<T> {
    pub fn new(
        renderer: T,
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
        entity_converter_registry: Arc<EntityConverterRegistry>,
    ) -> Self {
        Self {
            renderer,
            plugin_manager,
            cache_manager,
            entity_converter_registry,
        }
    }

    pub fn render_frame(
        &mut self,
        project_model: &ProjectModel,
        time: f64,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        let frame_info = self.get_frame(project_model, time);
        self.render_from_frame_info(&frame_info)
    }

    pub fn render_from_frame_info(
        &mut self,
        frame_info: &FrameInfo,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        self.clear()?;
        let object_count = frame_info.objects.len();
        let _timer = ScopedTimer::debug_lazy(|| {
            format!("RenderService::render_frame objects={}", object_count)
        });

        for frame_object in &frame_info.objects {
            let FrameObject {
                content,
                properties: _properties,
            } = frame_object;

            let scale = frame_info.render_scale.into_inner();

            // helper to scale transform and apply ROI offset
            let apply_view_transform = |t: &Transform, s: f64| -> Transform {
                let mut new_t = t.clone();
                let mut pos_x = t.position.x;
                let mut pos_y = t.position.y;

                if let Some(region) = &frame_info.region {
                    pos_x -= region.x;
                    pos_y -= region.y;
                }

                new_t.position.x = pos_x * s;
                new_t.position.y = pos_y * s;
                new_t.scale.x *= s;
                new_t.scale.y *= s;
                new_t
            };

            match content {
                FrameContent::Video {
                    surface,
                    frame_number,
                } => {
                    let request = LoadRequest::VideoFrame {
                        stream_index: None,
                        path: surface.file_path.clone(),
                        frame_number: *frame_number,
                        input_color_space: surface.input_color_space.clone(),
                        output_color_space: surface.output_color_space.clone(),
                    };
                    let video_frame = measure_debug_lazy(
                        || format!("Decode video {} frame {}", surface.file_path, frame_number),
                        || -> Result<Image, LibraryError> {
                            match self
                                .plugin_manager
                                .load_resource(&request, &self.cache_manager)?
                            {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    let final_image = self.apply_effects(
                        RenderOutput::Image(video_frame),
                        &surface.effects,
                        frame_info.now_time.0,
                    )?;
                    measure_debug_lazy(
                        || format!("Draw video {}", surface.file_path),
                        || {
                            self.renderer.draw_layer(
                                &final_image,
                                &apply_view_transform(&surface.transform, scale),
                            )
                        },
                    )?;
                }
                FrameContent::Image { surface } => {
                    let request = LoadRequest::Image {
                        path: surface.file_path.clone(),
                    };
                    let image_frame = measure_debug_lazy(
                        || format!("Load image {}", surface.file_path),
                        || -> Result<Image, LibraryError> {
                            match self
                                .plugin_manager
                                .load_resource(&request, &self.cache_manager)?
                            {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    let final_image = self.apply_effects(
                        RenderOutput::Image(image_frame),
                        &surface.effects,
                        frame_info.now_time.0,
                    )?;
                    measure_debug_lazy(
                        || format!("Draw image {}", surface.file_path),
                        || {
                            self.renderer.draw_layer(
                                &final_image,
                                &apply_view_transform(&surface.transform, scale),
                            )
                        },
                    )?;
                }
                FrameContent::Text {
                    text,
                    font,
                    size,
                    styles,
                    effects,
                    transform,
                } => {
                    let scaled_transform = apply_view_transform(transform, scale);
                    let text_layer =
                        measure_debug_lazy(|| format!("Rasterize text layer '{}'", text), || {
                            self.renderer.rasterize_text_layer(
                                &text,
                                *size,
                                &font,
                                &styles,
                                &scaled_transform,
                            )
                        })?;
                    let final_image =
                        self.apply_effects(text_layer, &effects, frame_info.now_time.0)?;
                    let mut composite_transform = Transform::default();
                    composite_transform.opacity = transform.opacity;
                    measure_debug_lazy(|| format!("Composite text '{}'", text), || {
                        self.renderer.draw_layer(&final_image, &composite_transform)
                    })?;
                }
                FrameContent::Shape {
                    path,
                    styles,
                    path_effects,
                    effects,
                    transform,
                } => {
                    let scaled_transform = apply_view_transform(transform, scale);
                    let shape_layer =
                        measure_debug_lazy(|| format!("Rasterize shape layer {}", path), || {
                            self.renderer.rasterize_shape_layer(
                                &path,
                                &styles,
                                &path_effects,
                                &scaled_transform,
                            )
                        })?;
                    let final_image =
                        self.apply_effects(shape_layer, &effects, frame_info.now_time.0)?;
                    let mut composite_transform = Transform::default();
                    composite_transform.opacity = transform.opacity;
                    measure_debug_lazy(|| format!("Composite shape {}", path), || {
                        self.renderer.draw_layer(&final_image, &composite_transform)
                    })?;
                }
                FrameContent::SkSL {
                    shader,
                    resolution,
                    effects,
                    transform,
                } => {
                    let scaled_transform = apply_view_transform(transform, scale);
                    let sksl_layer = measure_debug("Rasterize SkSL", || {
                        self.renderer.rasterize_sksl_layer(
                            &shader,
                            *resolution,
                            frame_info.now_time.0 as f32, // Use frame_info.now_time
                            &scaled_transform,
                        )
                    })?;
                    let final_image =
                        self.apply_effects(sksl_layer, &effects, frame_info.now_time.0)?;
                    let mut composite_transform = Transform::default();
                    composite_transform.opacity = transform.opacity;
                    measure_debug("Composite SkSL", || {
                        self.renderer.draw_layer(&final_image, &composite_transform)
                    })?;
                }
            }
        }
        measure_debug("RenderService::finalize", || self.renderer.finalize())
    }

    pub fn clear(&mut self) -> Result<(), LibraryError> {
        measure_debug("RenderService::clear", || self.renderer.clear())
    }

    fn get_frame(&self, project_model: &ProjectModel, time: f64) -> FrameInfo {
        let property_evaluators = self.plugin_manager.get_property_evaluators();

        let project = project_model.project();
        let composition_index = project_model.composition_index();
        let composition = &project.compositions[composition_index];
        let composition_fps = composition.fps;

        // Convert time (f64) to frame_number (u64) using composition_fps
        let frame_number = (time * composition_fps).round() as u64;

        get_frame_from_project(
            project,
            composition_index,
            frame_number, // Pass frame_number (u64)
            1.0,          // Default render_scale to 1.0 for self-managed renders (e.g. export)
            None,
            &property_evaluators,
            &self.entity_converter_registry,
        )
    }

    fn apply_effects(
        &mut self,
        layer: RenderOutput,
        effects: &[crate::model::frame::effect::ImageEffect],
        current_time: f64,
    ) -> Result<RenderOutput, LibraryError> {
        if effects.is_empty() {
            Ok(layer)
        } else {
            let mut current_layer = layer;
            // Iterate over effects
            for effect in effects {
                let effect_type = effect.effect_type.as_str();
                let gpu_context = self.renderer.get_gpu_context();

                let mut params = effect.properties.clone();
                params.insert(
                    "u_time".to_string(),
                    crate::model::project::property::PropertyValue::Number(
                        ordered_float::OrderedFloat(current_time),
                    ),
                );

                // Use the PluginManager to apply the effect
                current_layer = measure_debug_lazy(
                    || format!("Apply effect '{}'", effect_type),
                    || {
                        self.plugin_manager.apply_effect(
                            effect_type,
                            &current_layer,
                            &params,
                            gpu_context,
                        )
                    },
                )?;
            }
            Ok(current_layer)
        }
    }
} // Added closing brace for impl RenderService
