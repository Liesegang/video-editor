use crate::core::cache::SharedCacheManager;
use crate::core::framing::get_frame_from_project;
use crate::core::rendering::managed_color_backend::{
    ManagedRenderDestination, ProjectColorPipeline,
};
use crate::core::rendering::managed_color_source::ingest_loaded_media_from_assets;
use crate::core::rendering::media_color_ingress::{
    MediaAssetKind, require_unmanaged_abi_srgb, source_asset_from_assets,
};
use crate::core::rendering::renderer::{
    Affine2D, ParticleRasterRequest, RenderOutput, Renderer, RetainedRenderLayer,
    ShapeRasterRequest, SkSLRasterRequest, TextRasterRequest,
};
use crate::editor::project_model::ProjectModel;
use crate::error::{LibraryError, TransitionSourceHandleError};
use crate::model::asset::Asset;
use crate::model::authoring::AuthoringProject;
use crate::model::frame::entity::{
    FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject, FrameTransition,
    FrameTransitionKind, FrameTransitionSource, ImageSurface,
};
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
use crate::model::project::Project;
use crate::plugin::{ExportFrame, LoadRequest, PluginManager};
use crate::util::timing::{ScopedTimer, measure_debug};
use std::sync::Arc;

mod color_pipeline_cache;
mod transition;
use color_pipeline_cache::ProjectColorPipelineCache;

pub struct RenderService<T: Renderer> {
    pub renderer: T,
    cache_manager: SharedCacheManager,
    plugin_manager: Arc<PluginManager>,
    color_pipeline_cache: ProjectColorPipelineCache,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderDestination {
    Preview,
    Export,
}

enum RenderColorAuthority<'a> {
    Managed {
        assets: &'a [Asset],
        pipeline: &'a ProjectColorPipeline,
    },
    UnmanagedAbi,
}

fn prepare_effect_colors(
    params: &mut std::collections::HashMap<String, crate::model::property::PropertyValue>,
    color_parameters: &[String],
    color_authority: &RenderColorAuthority<'_>,
) -> Result<(), LibraryError> {
    let RenderColorAuthority::Managed { pipeline, .. } = color_authority else {
        return Ok(());
    };
    for name in color_parameters {
        let Some(value) = params.get_mut(name) else {
            continue;
        };
        let authored = match value {
            crate::model::property::PropertyValue::Color(color) => {
                crate::model::property::ColorValue::from_straight_srgba8(color)
            }
            crate::model::property::PropertyValue::ColorValue(color) => color.clone(),
            _ => continue,
        };
        *value = crate::model::property::PropertyValue::ColorValue(
            pipeline.effect_color_to_working(&authored)?,
        );
    }
    Ok(())
}

struct MediaRenderInput<'a> {
    request: &'a LoadRequest,
    surface: &'a ImageSurface,
    expected_kind: MediaAssetKind,
}

fn composition_frame_at_time(time: f64, composition_fps: f64) -> Result<u64, LibraryError> {
    if !time.is_finite() || time < 0.0 {
        return Err(LibraryError::Render(format!(
            "render time must be finite and non-negative, not {time}"
        )));
    }
    if !composition_fps.is_finite() || composition_fps <= 0.0 {
        return Err(LibraryError::Render(format!(
            "composition fps must be finite and positive, not {composition_fps}"
        )));
    }
    let scaled = time * composition_fps;
    if !scaled.is_finite() || scaled >= u64::MAX as f64 {
        return Err(LibraryError::Render(format!(
            "render frame position is outside the supported u64 range: {scaled}"
        )));
    }

    // Frame samples use interval semantics. Correct only arithmetic noise at
    // mathematically integral boundaries; a genuine half-frame such as 59.5
    // must still select frame 59 rather than rounding beyond the source end.
    let nearest = scaled.round();
    let snap_tolerance = f64::EPSILON * 8.0 * scaled.abs().max(1.0);
    let interval_position = if (scaled - nearest).abs() <= snap_tolerance {
        nearest
    } else {
        scaled
    };
    Ok(interval_position.floor() as u64)
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
            color_pipeline_cache: ProjectColorPipelineCache::new(),
        }
    }

    /// Render specifically for an exporter and retain the Project-derived
    /// color authority alongside the terminal pixels.
    pub fn render_export_frame(
        &mut self,
        project_model: &ProjectModel,
        time: f64,
    ) -> Result<ExportFrame, LibraryError> {
        let frame_info = self.get_frame(project_model, time)?;
        let output = self.render_project_frame(
            project_model.project().as_ref(),
            &frame_info,
            RenderDestination::Export,
        )?;
        let image = match output {
            RenderOutput::Image(image) => image,
            RenderOutput::Working(_) => {
                return Err(LibraryError::Render(
                    "export received an unterminated Project working frame".to_string(),
                ));
            }
            RenderOutput::Texture(_) => {
                return Err(LibraryError::Render(
                    "export received a GPU texture without a typed readback boundary".to_string(),
                ));
            }
        };
        ExportFrame::from_project_render(project_model.project().as_ref(), image)
    }

    /// Render an authoring Timeline frame for an exporter and retain the exact
    /// authoring Project color authority with the terminal pixels.
    pub fn render_authoring_export_frame(
        &mut self,
        project: &AuthoringProject,
        frame_info: &FrameInfo,
    ) -> Result<ExportFrame, LibraryError> {
        let output = self.render_authoring_frame(project, frame_info, RenderDestination::Export)?;
        let image = match output {
            RenderOutput::Image(image) => image,
            RenderOutput::Working(_) => {
                return Err(LibraryError::Render(
                    "authoring export received an unterminated working frame".to_string(),
                ));
            }
            RenderOutput::Texture(_) => {
                return Err(LibraryError::Render(
                    "authoring export received a GPU texture without a typed readback boundary"
                        .to_string(),
                ));
            }
        };
        ExportFrame::from_authoring_render(project, image)
    }

    /// Render a Project-evaluated frame with its exact color and Asset
    /// authority. Preview and export must use this entry point.
    pub fn render_project_frame(
        &mut self,
        project: &Project,
        frame_info: &FrameInfo,
        destination: RenderDestination,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        let managed_destination = match destination {
            RenderDestination::Preview => ManagedRenderDestination::Preview,
            RenderDestination::Export => ManagedRenderDestination::Export,
        };
        let pipeline = self
            .color_pipeline_cache
            .for_project(project, managed_destination)?;
        self.renderer
            .use_project_linear_surface(pipeline.working_surface_contract())?;
        let output = self.render_with_authority(
            frame_info,
            &RenderColorAuthority::Managed {
                assets: &project.assets,
                pipeline: pipeline.as_ref(),
            },
        )?;
        let RenderOutput::Working(working) = output else {
            return Err(LibraryError::Render(
                "Project renderer did not retain the typed linear RGBAF32 root output".to_string(),
            ));
        };
        pipeline.terminal_image(&working).map(RenderOutput::Image)
    }

    /// Rasterize an authoring Timeline frame under the project
    /// Project's exact color and Asset authority. Frame evaluation remains a
    /// separate RenderPlan step so asynchronous workers can return the same
    /// `FrameInfo` they actually rendered.
    pub fn render_authoring_frame(
        &mut self,
        project: &AuthoringProject,
        frame_info: &FrameInfo,
        destination: RenderDestination,
    ) -> Result<RenderOutput, LibraryError> {
        let managed_destination = match destination {
            RenderDestination::Preview => ManagedRenderDestination::Preview,
            RenderDestination::Export => ManagedRenderDestination::Export,
        };
        let pipeline = self
            .color_pipeline_cache
            .for_authoring_project(project, managed_destination)?;
        self.renderer
            .use_project_linear_surface(pipeline.working_surface_contract())?;
        let output = self.render_with_authority(
            frame_info,
            &RenderColorAuthority::Managed {
                assets: &project.assets,
                pipeline: pipeline.as_ref(),
            },
        )?;
        let RenderOutput::Working(working) = output else {
            return Err(LibraryError::Render(
                "Authoring renderer did not retain the typed linear RGBAF32 root output"
                    .to_string(),
            ));
        };
        pipeline.terminal_image(&working).map(RenderOutput::Image)
    }

    /// Project-free compatibility boundary for versioned native plugin probes.
    /// File-backed Preview/export must use [`Self::render_project_frame`].
    pub fn render_from_frame_info(
        &mut self,
        frame_info: &FrameInfo,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        self.renderer.use_unmanaged_srgba8_surface()?;
        self.render_with_authority(frame_info, &RenderColorAuthority::UnmanagedAbi)
    }

    fn render_with_authority(
        &mut self,
        frame_info: &FrameInfo,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        self.clear()?;
        let object_count = frame_info.object_count();
        let _timer = ScopedTimer::debug(format!(
            "RenderService::render_frame objects={}",
            object_count
        ));
        let context = RenderContext::root(frame_info);
        self.render_items(
            &frame_info.items,
            &context,
            frame_info.now_time.into_inner(),
            color_authority,
        )?;
        measure_debug("RenderService::finalize", || self.renderer.finalize())
    }

    fn render_items(
        &mut self,
        items: &[FrameItem],
        context: &RenderContext,
        current_time: f64,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        for item in items {
            match item {
                FrameItem::Object(object) => {
                    self.render_object(object, context, current_time, color_authority)?;
                }
                FrameItem::Group(group) => self.render_group(group, context, color_authority)?,
                FrameItem::Transition(transition) => {
                    self.render_transition(transition, context, color_authority)?;
                }
            }
        }
        Ok(())
    }

    fn render_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        if group.kind == FrameGroupKind::Composition {
            return self.render_composition_group(group, parent_context, color_authority);
        }
        if group.kind == FrameGroupKind::ImageTransform {
            return self.render_image_transform_group(group, parent_context, color_authority);
        }
        let child_context = parent_context.with_transform(&group.transform);
        if !group_requires_isolation(group) {
            return self.render_items(
                &group.items,
                &child_context,
                group.effect_time.into_inner(),
                color_authority,
            );
        }

        self.renderer.begin_group(
            parent_context.target_width,
            parent_context.target_height,
            &transparent_color(),
        )?;
        let children_result = self.render_items(
            &group.items,
            &child_context,
            group.effect_time.into_inner(),
            color_authority,
        );
        let output_result = self.renderer.end_group();
        children_result?;
        let output = output_result?;
        let output = self.apply_effects(
            output,
            &group.effects,
            group.effect_time.into_inner(),
            color_authority,
        )?;
        self.renderer.draw_layer_affine_with_blend(
            &output,
            &Affine2D::IDENTITY,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    /// Rasterize the upstream Image subtree before its affine transform,
    /// preserving graph order and treating descendants as one image.
    fn render_image_transform_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let width = scaled_dimension(group.width as f64, parent_context.render_scale);
        let height = scaled_dimension(group.height as f64, parent_context.render_scale);
        let child_context = RenderContext::composition(parent_context.render_scale, width, height);

        self.renderer
            .begin_group(width, height, &transparent_color())?;
        let children_result = self.render_items(
            &group.items,
            &child_context,
            group.effect_time.into_inner(),
            color_authority,
        );
        let output_result = self.renderer.end_group();
        children_result?;
        let output = output_result?;

        let pixel_to_local = Affine2D::scale(
            1.0 / parent_context.render_scale,
            1.0 / parent_context.render_scale,
        );
        let transform = parent_context
            .logical_to_target
            .compose(Affine2D::from(&group.transform))
            .compose(pixel_to_local);
        self.renderer.draw_layer_affine_with_blend(
            &output,
            &transform,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    fn render_composition_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let width = scaled_dimension(group.width as f64, parent_context.render_scale);
        let height = scaled_dimension(group.height as f64, parent_context.render_scale);
        let child_context = RenderContext::composition(parent_context.render_scale, width, height);

        self.renderer
            .begin_group(width, height, &group.background_color)?;
        let children_result = self.render_items(
            &group.items,
            &child_context,
            group.effect_time.into_inner(),
            color_authority,
        );
        let output_result = self.renderer.end_group();
        children_result?;
        let output = output_result?;
        let output = self.apply_effects(
            output,
            &group.effects,
            group.effect_time.into_inner(),
            color_authority,
        )?;

        let pixel_to_local = Affine2D::scale(
            1.0 / parent_context.render_scale,
            1.0 / parent_context.render_scale,
        );
        let transform = parent_context
            .logical_to_target
            .compose(Affine2D::from(&group.transform))
            .compose(pixel_to_local);
        self.renderer.draw_layer_affine_with_blend(
            &output,
            &transform,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    fn render_object(
        &mut self,
        frame_object: &FrameObject,
        context: &RenderContext,
        current_time: f64,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let content = &frame_object.content;

        match content {
            FrameContent::Video {
                surface,
                source_time,
                stream_index,
            } => {
                let source_color_authority = match color_authority {
                    RenderColorAuthority::Managed { assets, .. } => {
                        source_asset_from_assets(assets, surface, MediaAssetKind::Video)?
                            .and_then(|asset| asset.source_color.decoder_color_authority())
                    }
                    RenderColorAuthority::UnmanagedAbi => None,
                };
                let request = LoadRequest::VideoFrame {
                    path: surface.file_path.clone(),
                    source_time: *source_time,
                    stream_index: *stream_index,
                    source_color_authority,
                };
                self.render_media_surface(
                    MediaRenderInput {
                        request: &request,
                        surface,
                        expected_kind: MediaAssetKind::Video,
                    },
                    context,
                    current_time,
                    color_authority,
                )
            }
            FrameContent::Image { surface } => {
                let request = LoadRequest::Image {
                    path: surface.file_path.clone(),
                };
                self.render_media_surface(
                    MediaRenderInput {
                        request: &request,
                        surface,
                        expected_kind: MediaAssetKind::Image,
                    },
                    context,
                    current_time,
                    color_authority,
                )
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
                let render_transform = context.transform(transform);
                let text_layer = measure_debug(format!("Rasterize text layer '{}'", text), || {
                    self.renderer.rasterize_text_layer(TextRasterRequest {
                        text,
                        size: *size,
                        font_name: font,
                        styles,
                        ensemble: ensemble.as_ref(),
                        transform: render_transform,
                        current_time,
                    })
                })?;
                let final_image =
                    self.apply_effects(text_layer, effects, current_time, color_authority)?;
                measure_debug(format!("Composite text '{}'", text), || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
            FrameContent::Shape {
                path,
                canonical_path,
                styles,
                path_effects,
                effects,
                ensemble,
                transform,
            } => {
                let render_transform = context.transform(transform);
                let shape_layer = measure_debug(format!("Rasterize shape layer {}", path), || {
                    self.renderer.rasterize_shape_layer(ShapeRasterRequest {
                        path_data: path,
                        canonical_path: canonical_path.as_ref(),
                        styles,
                        path_effects,
                        ensemble: ensemble.as_ref(),
                        transform: render_transform,
                    })
                })?;
                let final_image =
                    self.apply_effects(shape_layer, effects, current_time, color_authority)?;
                measure_debug(format!("Composite shape {}", path), || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
            FrameContent::SkSL {
                shader,
                resolution,
                color_domain,
                effects,
                transform,
            } => {
                let render_transform = context.transform(transform);
                let sksl_layer = measure_debug("Rasterize SkSL", || {
                    self.renderer.rasterize_sksl_layer(SkSLRasterRequest {
                        shader_code: shader,
                        resolution: *resolution,
                        time: current_time as f32,
                        transform: &render_transform,
                        color_domain: *color_domain,
                    })
                })?;
                let final_image =
                    self.apply_effects(sksl_layer, effects, current_time, color_authority)?;
                measure_debug("Composite SkSL", || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
            FrameContent::ParticleScene {
                scene,
                effects,
                transform,
            } => {
                let render_transform = context.transform(transform);
                let particle_layer = measure_debug("Rasterize GPU Particle scene", || {
                    self.renderer
                        .rasterize_particle_layer(ParticleRasterRequest {
                            scene,
                            transform: &render_transform,
                        })
                })?;
                let final_image =
                    self.apply_effects(particle_layer, effects, current_time, color_authority)?;
                measure_debug("Composite GPU Particle scene", || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
        }
    }
    pub fn clear(&mut self) -> Result<(), LibraryError> {
        measure_debug("RenderService::clear", || self.renderer.clear())
    }

    fn get_frame(
        &self,
        project_model: &ProjectModel,
        time: f64,
    ) -> Result<FrameInfo, LibraryError> {
        let property_evaluators = self.plugin_manager.get_property_evaluators();

        let project = project_model.project();
        let composition_index = project_model.composition_index();
        let composition = &project.compositions[composition_index];
        let composition_fps = composition.fps;

        let frame_number = composition_frame_at_time(time, composition_fps)?;

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
        effects: &[crate::model::frame::effect::ImageEffect],
        current_time: f64,
        color_authority: &RenderColorAuthority<'_>,
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
                if matches!(current_layer, RenderOutput::Working(_)) {
                    let color_parameters = self
                        .plugin_manager
                        .effect_project_linear_color_parameters(effect_type);
                    prepare_effect_colors(&mut params, &color_parameters, color_authority)?;
                }
                params.insert(
                    "u_time".to_string(),
                    crate::model::property::PropertyValue::Number(ordered_float::OrderedFloat(
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
        input: MediaRenderInput<'_>,
        context: &RenderContext,
        current_time: f64,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let MediaRenderInput {
            request,
            surface,
            expected_kind,
        } = input;
        let response = measure_debug(format!("Load {}", surface.file_path), || {
            self.plugin_manager
                .load_resource(request, &self.cache_manager)
        })?;
        let layer = match color_authority {
            RenderColorAuthority::Managed {
                assets, pipeline, ..
            } => {
                let working = ingest_loaded_media_from_assets(
                    assets,
                    pipeline,
                    surface,
                    expected_kind,
                    response,
                )?;
                RenderOutput::Working(working)
            }
            RenderColorAuthority::UnmanagedAbi => {
                require_unmanaged_abi_srgb(response.decoded(), response.pixels())?;
                RenderOutput::Image(response.into_rgba8()?)
            }
        };

        let final_image =
            self.apply_effects(layer, &surface.effects, current_time, color_authority)?;

        let render_transform = context.transform(&surface.transform);

        measure_debug(format!("Draw {}", surface.file_path), || {
            self.renderer.draw_layer_affine_with_blend(
                &final_image,
                &render_transform,
                surface.transform.opacity,
                crate::model::BlendMode::Normal,
            )
        })?;

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RenderContext {
    logical_to_target: Affine2D,
    render_scale: f64,
    target_width: u32,
    target_height: u32,
}

impl RenderContext {
    fn root(frame: &FrameInfo) -> Self {
        let render_scale = frame.render_scale.into_inner().max(f64::EPSILON);
        let (region_x, region_y, logical_width, logical_height) = frame.region.as_ref().map_or(
            (0.0, 0.0, frame.width as f64, frame.height as f64),
            |region| (region.x, region.y, region.width, region.height),
        );
        let logical_to_target = Affine2D::scale(render_scale, render_scale)
            .compose(Affine2D::translate(-region_x, -region_y));
        Self {
            logical_to_target,
            render_scale,
            target_width: scaled_dimension(logical_width, render_scale),
            target_height: scaled_dimension(logical_height, render_scale),
        }
    }

    fn composition(render_scale: f64, target_width: u32, target_height: u32) -> Self {
        Self {
            logical_to_target: Affine2D::scale(render_scale, render_scale),
            render_scale,
            target_width,
            target_height,
        }
    }

    fn with_transform(self, transform: &Transform) -> Self {
        Self {
            logical_to_target: self.logical_to_target.compose(Affine2D::from(transform)),
            ..self
        }
    }

    fn transform(self, transform: &Transform) -> Affine2D {
        self.logical_to_target.compose(Affine2D::from(transform))
    }
}

fn scaled_dimension(logical: f64, scale: f64) -> u32 {
    (logical * scale).round().clamp(1.0, u32::MAX as f64) as u32
}

fn transparent_color() -> crate::model::frame::color::Color {
    crate::model::frame::color::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn group_requires_isolation(group: &FrameGroup) -> bool {
    group.kind == FrameGroupKind::Merge
        || group.kind == FrameGroupKind::TransitionOutput
        || !group.effects.is_empty()
        || group.blend_mode != crate::model::BlendMode::Normal
        || (group.transform.opacity - 1.0).abs() > f64::EPSILON
}

#[cfg(test)]
#[path = "render_service_tests.rs"]
mod tests;
