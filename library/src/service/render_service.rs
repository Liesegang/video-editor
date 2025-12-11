use crate::cache::SharedCacheManager;
use crate::error::LibraryError;
use crate::framing::entity_converters::EntityConverterRegistry;
use crate::framing::get_frame_from_project;
use crate::loader::image::Image;
use crate::model::frame::entity::{FrameContent, FrameObject}; // FrameEntity -> FrameContent
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
use crate::plugin::{LoadRequest, LoadResponse, PluginManager};
use crate::rendering::renderer::Renderer;
use crate::service::project_model::ProjectModel;
use crate::util::timing::{ScopedTimer, measure_debug}; // Added this line
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
    ) -> Result<Image, LibraryError> {
        self.clear()?;
        let frame_info = self.get_frame(project_model, time);
        let object_count = frame_info.objects.len();
        let _timer = ScopedTimer::debug(format!(
            "RenderService::render_frame objects={}",
            object_count
        ));

        for frame_object in frame_info.objects {
            let FrameObject {
                content, // entity -> content
                properties: _properties,
            } = frame_object;
            match content { // entity -> content
                FrameContent::Video { // FrameEntity -> FrameContent
                    surface,
                    frame_number,
                } => {
                    let request = LoadRequest::VideoFrame {
                        path: surface.file_path.clone(),
                        frame_number,
                    };
                    let video_frame = measure_debug(
                        format!("Decode video {} frame {}", surface.file_path, frame_number),
                        || -> Result<Image, LibraryError> {
                            match self
                                .plugin_manager
                                .load_resource(&request, &self.cache_manager)?
                            {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    let final_image = self.apply_effects(video_frame, &surface.effects, time)?;
                    measure_debug(format!("Draw video {}", surface.file_path), || {
                        self.renderer.draw_image(&final_image, &surface.transform)
                    })?;
                }
                FrameContent::Image { surface } => { // FrameEntity -> FrameContent
                    let request = LoadRequest::Image {
                        path: surface.file_path.clone(),
                    };
                    let image_frame = measure_debug(
                        format!("Load image {}", surface.file_path),
                        || -> Result<Image, LibraryError> {
                            match self
                                .plugin_manager
                                .load_resource(&request, &self.cache_manager)?
                            {
                                LoadResponse::Image(img) => Ok(img),
                            }
                        },
                    )?;
                    let final_image = self.apply_effects(image_frame, &surface.effects, time)?;
                    measure_debug(format!("Draw image {}", surface.file_path), || {
                        self.renderer.draw_image(&final_image, &surface.transform)
                    })?;
                }
                FrameContent::Text { // FrameEntity -> FrameContent
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
                    let final_image = self.apply_effects(text_layer, &effects, time)?;
                    measure_debug(format!("Composite text '{}'", text), || {
                        self.renderer
                            .draw_image(&final_image, &Transform::default())
                    })?;
                }
                FrameContent::Shape { // FrameEntity -> FrameContent
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
                    let final_image = self.apply_effects(shape_layer, &effects, time)?;
                    measure_debug(format!("Composite shape {}", path), || {
                        self.renderer
                            .draw_image(&final_image, &Transform::default())
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
            &property_evaluators,
            &self.entity_converter_registry,
        )
    }

    fn apply_effects(
        &self,
        mut image: Image,
        effects: &[crate::model::frame::effect::ImageEffect],
        _current_time: f64, // Not prefixed with _ anymore
    ) -> Result<Image, LibraryError> {
        if effects.is_empty() {
            Ok(image)
        } else {
            for effect in effects {
                // The 'ImageEffect' struct already holds evaluated 'PropertyValue's.
                // We just need to pass them to the plugin manager.
                image = measure_debug(format!("Apply effect '{}'", effect.effect_type), || {
                    self.plugin_manager.apply_effect(
                        effect.effect_type.as_str(),
                        &image,
                        &effect.properties,
                    )
                })?;
            }
            Ok(image)
        }
    }
} // Added closing brace for impl RenderService
