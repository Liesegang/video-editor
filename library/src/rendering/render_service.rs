use crate::error::LibraryError;
use crate::plugin::{LoadRequest, PluginManager};
use crate::rendering::cache::SharedCacheManager;
use crate::rendering::framing::get_frame_from_project;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::runtime::entity::{FrameContent, FrameObject};
use crate::runtime::frame::FrameInfo;
use crate::runtime::transform::Transform;
use crate::service::project_model::ProjectModel;
use crate::timing::{ScopedTimer, measure_debug};
use std::sync::Arc;

pub struct RenderService<T: Renderer> {
    pub(crate) renderer: T,
    cache_manager: SharedCacheManager,
    plugin_manager: Arc<PluginManager>,
}

impl<T: Renderer> RenderService<T> {
    pub fn new(
        renderer: T,
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
    ) -> Self {
        Self {
            renderer,
            plugin_manager,
            cache_manager,
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

    pub(crate) fn render_from_frame_info(
        &mut self,
        frame_info: &FrameInfo,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        self.clear()?;
        let object_count = frame_info.objects.len();
        let _timer = ScopedTimer::debug(format!(
            "RenderService::render_frame objects={}",
            object_count
        ));

        for frame_object in &frame_info.objects {
            let FrameObject {
                content,
                properties: _properties,
            } = frame_object;

            let scale = frame_info.render_scale.into_inner();
            let region = frame_info.region.as_ref();

            match content {
                FrameContent::Video {
                    surface,
                    frame_number,
                } => {
                    let request = LoadRequest::VideoFrame {
                        path: surface.file_path.clone(),
                        frame_number: *frame_number,
                        stream_index: None,
                        input_color_space: surface.input_color_space.clone(),
                        output_color_space: surface.output_color_space.clone(),
                    };
                    self.render_media_surface(
                        &request,
                        &surface.file_path,
                        &surface.transform,
                        &surface.effects,
                        frame_info,
                        scale,
                    )?;
                }
                FrameContent::Image { surface } => {
                    let request = LoadRequest::Image {
                        path: surface.file_path.clone(),
                    };
                    self.render_media_surface(
                        &request,
                        &surface.file_path,
                        &surface.transform,
                        &surface.effects,
                        frame_info,
                        scale,
                    )?;
                }
                FrameContent::Text {
                    text,
                    font,
                    size,
                    styles,
                    effects,
                    ensemble,
                    transform,
                } => {
                    let scaled_transform = apply_view_transform(transform, region, scale);
                    let text_layer =
                        measure_debug(format!("Rasterize text layer '{}'", text), || {
                            self.renderer.rasterize_text_layer(
                                &text,
                                *size,
                                &font,
                                &styles,
                                ensemble.as_ref(),
                                &scaled_transform,
                            )
                        })?;
                    let final_image =
                        self.apply_effects(text_layer, &effects, frame_info.now_time.0)?;
                    let mut composite_transform = Transform::default();
                    composite_transform.opacity = transform.opacity;
                    measure_debug(format!("Composite text '{}'", text), || {
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
                    let scaled_transform = apply_view_transform(transform, region, scale);
                    let shape_layer =
                        measure_debug(format!("Rasterize shape layer {}", path), || {
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
                    measure_debug(format!("Composite shape {}", path), || {
                        self.renderer.draw_layer(&final_image, &composite_transform)
                    })?;
                }
                FrameContent::SkSL {
                    shader,
                    resolution,
                    effects,
                    transform,
                } => {
                    let scaled_transform = apply_view_transform(transform, region, scale);
                    let sksl_layer = measure_debug(format!("Rasterize SkSL"), || {
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
                    measure_debug(format!("Composite SkSL"), || {
                        self.renderer.draw_layer(&final_image, &composite_transform)
                    })?;
                }
            }
        }
        measure_debug("RenderService::finalize", || self.renderer.finalize())
    }

    pub(crate) fn clear(&mut self) -> Result<(), LibraryError> {
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
            &self.plugin_manager,
        )
    }

    fn apply_effects(
        &mut self,
        layer: RenderOutput,
        effects: &[crate::runtime::effect::ImageEffect],
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
                    crate::project::property::PropertyValue::Number(ordered_float::OrderedFloat(
                        current_time,
                    )),
                );

                // Use the PluginManager to apply the effect
                current_layer = measure_debug(format!("Apply effect '{}'", effect_type), || {
                    self.plugin_manager.apply_effect(
                        effect_type,
                        &current_layer,
                        &params,
                        gpu_context,
                    )
                })?;
            }
            Ok(current_layer)
        }
    }

    /// Helper to load, apply effects, and draw a media surface (video or image).
    fn render_media_surface(
        &mut self,
        request: &LoadRequest,
        file_path: &str,
        transform: &Transform,
        effects: &[crate::runtime::effect::ImageEffect],
        frame_info: &FrameInfo,
        scale: f64,
    ) -> Result<(), LibraryError> {
        let image = measure_debug(format!("Load {}", file_path), || {
            self.plugin_manager
                .load_resource(request, &self.cache_manager)
                .map(|r| r.image)
        })?;

        let final_image =
            self.apply_effects(RenderOutput::Image(image), effects, frame_info.now_time.0)?;

        let view_transform = apply_view_transform(transform, frame_info.region.as_ref(), scale);

        measure_debug(format!("Draw {}", file_path), || {
            self.renderer.draw_layer(&final_image, &view_transform)
        })?;

        Ok(())
    }
}

/// Apply scale and optional region offset to a transform.
fn apply_view_transform(
    t: &Transform,
    region: Option<&crate::runtime::frame::Region>,
    scale: f64,
) -> Transform {
    let mut new_t = t.clone();
    let mut pos_x = t.position.x;
    let mut pos_y = t.position.y;

    if let Some(r) = region {
        pos_x -= r.x;
        pos_y -= r.y;
    }

    new_t.position.x = pos_x * scale;
    new_t.position.y = pos_y * scale;
    new_t.scale.x *= scale;
    new_t.scale.y *= scale;
    new_t
}
