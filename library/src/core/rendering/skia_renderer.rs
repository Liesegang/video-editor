use crate::cache::SharedCacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::SkSLColorDomain;
use crate::model::frame::runtime_shape::evaluate_text_element_transforms;
use crate::rendering::blend::{BlendRuntime, with_restored_canvas};
use crate::rendering::renderer::{
    Affine2D, RenderOutput, Renderer, ShapeRasterRequest, SkSLRasterRequest, TextRasterRequest,
    TextureInfo, WorkingSurfaceContract,
};
use crate::rendering::shader_utils::{self, ShaderContext};
use crate::rendering::skia_utils::{
    GpuContext, create_gpu_context, create_image_from_texture, image_to_skia,
};
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};
use crate::rendering::text_layout::{build_text_paragraph, layout_runtime_text_shape};
use crate::util::timing::ScopedTimer;
use log::debug;

use skia_safe::{
    AlphaType, Canvas, ColorType, CubicResampler, ISize, ImageInfo, Matrix, Paint, Point,
    SamplingOptions, Shader, Surface, runtime_effect::ChildPtr,
};

mod legacy_backplate;
mod paint;

use paint::{PaintFactory, StrokeRenderConfig};

const SKSL_STRAIGHT_TO_PREMULTIPLIED: &str = r#"
uniform shader straight_input;

half4 main(float2 position) {
    half4 straight = straight_input.eval(position);
    return half4(straight.rgb * straight.a, straight.a);
}
"#;

pub struct SkiaRenderer {
    width: u32,
    height: u32,
    background_color: Color,
    surface: Surface,
    surface_contract: SkiaSurfaceContract,
    group_surfaces: Vec<GroupSurface>,
    blend_runtime: BlendRuntime,
    sksl_straight_to_premultiplied: Option<skia_safe::RuntimeEffect>,
    gpu_context: Option<GpuContext>,
    sharing_handle: Option<usize>,
    sharing_hwnd: Option<isize>,
}

struct GroupSurface {
    width: u32,
    height: u32,
    surface: Surface,
}

impl SkiaRenderer {
    pub fn render_to_texture(&mut self) -> Result<TextureInfo, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::render_to_texture");
        if self.surface_contract.working().is_some() {
            return Err(LibraryError::Render(
                "Project linear surface cannot be exposed as an untyped GPU TextureInfo"
                    .to_string(),
            ));
        }
        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();

            // Get the backend texture from the surface if possible
            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            ) && let Some(gl_info) = texture.gl_texture_info()
            {
                return Ok(TextureInfo {
                    texture_id: gl_info.id,
                    width: self.width,
                    height: self.height,
                });
            }
            Err(LibraryError::Render(
                "Failed to get GL texture info".to_string(),
            ))
        } else {
            Err(LibraryError::Render(
                "GPU context not available".to_string(),
            ))
        }
    }

    fn snapshot_surface(
        &self,
        surface: &mut Surface,
        width: u32,
        height: u32,
    ) -> Result<RenderOutput, LibraryError> {
        // These surfaces are local temporaries. Returning their backend texture
        // ID would leave a dangling RenderOutput as soon as the Surface drops.
        // The root surface remains alive and may still be finalized as a GPU
        // texture for Preview; transient layers cross this boundary as Images.
        skia_working_surface::snapshot_surface(surface, width, height, &self.surface_contract)
    }
}

impl SkiaRenderer {
    pub fn new(
        width: u32,
        height: u32,
        background_color: Color,
        use_gpu: bool,
        existing_context: Option<GpuContext>,
        _cache_manager: Option<SharedCacheManager>,
    ) -> Result<Self, LibraryError> {
        let mut gpu_context = if use_gpu {
            if let Some(mut ctx) = existing_context {
                debug!("SkiaRenderer: Reusing existing GPU context");
                ctx.resize(width, height);
                Some(ctx)
            } else if let Some(mut ctx) = create_gpu_context(None, None) {
                debug!("SkiaRenderer: Created new GPU context");
                ctx.resize(width, height);
                Some(ctx)
            } else {
                debug!("SkiaRenderer: GPU context creation failed, falling back to CPU");
                None
            }
        } else {
            None
        };

        if gpu_context.is_some() {
            debug!("SkiaRenderer: GPU context enabled");
        } else {
            debug!("SkiaRenderer: using CPU raster surfaces");
        }

        let surface_contract = SkiaSurfaceContract::UnmanagedSrgba8;
        let surface = skia_working_surface::create_surface(
            width,
            height,
            gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
            &surface_contract,
        )
        .map_err(|error| {
            LibraryError::Render(format!(
                "Cannot create Skia surface {width}x{height}: {error}"
            ))
        })?;

        let mut renderer = SkiaRenderer {
            width,
            height,
            background_color,
            surface,
            surface_contract,
            group_surfaces: Vec::new(),
            blend_runtime: BlendRuntime::new(),
            sksl_straight_to_premultiplied: None,
            gpu_context,
            sharing_handle: None,
            sharing_hwnd: None,
        };
        renderer.clear().map_err(|error| {
            LibraryError::Render(format!("Failed to clear render target: {error}"))
        })?;
        Ok(renderer)
    }

    pub fn resize_render_target(
        &mut self,
        width: u32,
        height: u32,
        background_color: Color,
    ) -> Result<(), LibraryError> {
        let mut surface = skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &self.surface_contract,
        )
        .map_err(|error| {
            LibraryError::Render(format!(
                "Cannot resize Skia surface to {width}x{height}: {error}"
            ))
        })?;
        skia_working_surface::clear_authored_color(
            &mut surface,
            &self.surface_contract,
            &background_color,
        )?;
        if let Some(context) = self.gpu_context.as_mut() {
            context.resize(width, height);
        }

        self.width = width;
        self.height = height;
        self.background_color = background_color;
        self.surface = surface;
        self.group_surfaces.clear();
        Ok(())
    }

    fn create_layer_surface(&mut self) -> Result<Surface, LibraryError> {
        let (width, height) = self.current_target_dimensions();
        skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
            &self.surface_contract,
        )
    }

    fn premultiply_straight_sksl_shader(
        &mut self,
        straight_shader: Shader,
    ) -> Result<Shader, LibraryError> {
        if self.sksl_straight_to_premultiplied.is_none() {
            self.sksl_straight_to_premultiplied = Some(
                skia_safe::RuntimeEffect::make_for_shader(SKSL_STRAIGHT_TO_PREMULTIPLIED, None)
                    .map_err(|error| {
                        LibraryError::Render(format!(
                            "Failed to compile the straight-alpha SkSL storage adapter: {error}"
                        ))
                    })?,
            );
        }
        let effect = self
            .sksl_straight_to_premultiplied
            .as_ref()
            .ok_or_else(|| {
                LibraryError::Render(
                    "Straight-alpha SkSL storage adapter remained unavailable".to_string(),
                )
            })?;
        effect
            .make_shader(
                skia_safe::Data::new_copy(&[]),
                &[ChildPtr::from(straight_shader)],
                None,
            )
            .ok_or_else(|| {
                LibraryError::Render(
                    "Failed to adapt straight-alpha SkSL to premultiplied storage".to_string(),
                )
            })
    }

    fn replace_surface_contract(
        &mut self,
        contract: SkiaSurfaceContract,
    ) -> Result<(), LibraryError> {
        if self.surface_contract.same_storage_contract(&contract) {
            self.surface_contract = contract;
            return Ok(());
        }
        let mut surface = skia_working_surface::create_surface(
            self.width,
            self.height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &contract,
        )?;
        skia_working_surface::clear_authored_color(
            &mut surface,
            &contract,
            &self.background_color,
        )?;
        self.surface = surface;
        self.surface_contract = contract;
        self.group_surfaces.clear();
        Ok(())
    }

    fn current_target_dimensions(&self) -> (u32, u32) {
        self.group_surfaces
            .last()
            .map(|group| (group.width, group.height))
            .unwrap_or((self.width, self.height))
    }

    fn replace_render_target(
        &mut self,
        mut gpu_context: Option<GpuContext>,
        sharing_handle: Option<usize>,
        sharing_hwnd: Option<isize>,
        create: impl FnOnce(Option<&mut skia_safe::gpu::DirectContext>) -> Result<Surface, LibraryError>,
    ) -> Result<(), LibraryError> {
        let mut surface = create(
            gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
        )?;
        skia_working_surface::clear_authored_color(
            &mut surface,
            &self.surface_contract,
            &self.background_color,
        )?;
        self.surface = surface;
        self.gpu_context = gpu_context;
        self.sharing_handle = sharing_handle;
        self.sharing_hwnd = sharing_hwnd;
        self.group_surfaces.clear();
        Ok(())
    }

    /// Render text with ensemble effectors and decorators.
    fn rasterize_ensemble_text(
        &mut self,
        request: TextRasterRequest<'_>,
        ensemble_data: &crate::core::ensemble::EnsembleData,
    ) -> Result<RenderOutput, LibraryError> {
        use crate::core::ensemble::target::EffectorTarget;
        use crate::core::ensemble::types::EffectorConfig;

        let TextRasterRequest {
            text,
            size,
            font_name,
            styles,
            transform,
            current_time,
            ..
        } = request;
        let current_time = current_time as f32;

        log::debug!(
            "Ensemble rendering: {} effectors, {} decorators",
            ensemble_data.effector_configs.len(),
            ensemble_data.decorator_configs.len()
        );

        for config in &ensemble_data.effector_configs {
            let target = match config {
                EffectorConfig::Transform { target, .. }
                | EffectorConfig::StepDelay { target, .. }
                | EffectorConfig::Opacity { target, .. }
                | EffectorConfig::Randomize { target, .. } => target,
            };
            if *target == EffectorTarget::Parts {
                return Err(LibraryError::Render(
                    "Ensemble EffectorTarget::Parts is not supported".to_string(),
                ));
            }
        }
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            let matrix = build_transform_matrix(&transform);
            canvas.save();
            canvas.concat(&matrix);

            let font_mgr = skia_safe::FontMgr::default();
            let typeface = font_mgr
                .match_family_style(font_name, skia_safe::FontStyle::default())
                .or_else(|| font_mgr.legacy_make_typeface(None, skia_safe::FontStyle::default()))
                .ok_or_else(|| {
                    LibraryError::Render(format!(
                        "No usable font was found for Ensemble text family {font_name:?}"
                    ))
                })?;
            let font = skia_safe::Font::from_typeface(typeface, size as f32);
            let runtime_text = layout_runtime_text_shape(text, font_name, size as f32);
            let elements = &runtime_text.elements;

            let character_transforms =
                evaluate_text_element_transforms(&runtime_text, ensemble_data, current_time)?;

            legacy_backplate::draw_text_backplates(
                canvas,
                &runtime_text,
                &character_transforms,
                &ensemble_data.decorator_configs,
                &self.surface_contract,
            )?;

            for (character, character_transform) in elements.iter().zip(&character_transforms) {
                let center = Point::new(
                    character.bounds.left + character.advance / 2.0,
                    (character.bounds.top + character.bounds.bottom) / 2.0,
                );
                canvas.save();
                canvas.translate((center.x, center.y));
                canvas.translate(character_transform.translate);
                canvas.rotate(character_transform.rotate, None);
                canvas.scale(character_transform.scale);
                canvas.translate((-center.x, -center.y));

                for config in styles {
                    let paint = PaintFactory::new(&self.surface_contract).text_paint(
                        &config.style,
                        character_transform.opacity,
                        character_transform.color_override.as_ref(),
                    )?;
                    // TODO: draw SkParagraph shaping runs with source mapping.
                    // Per-grapheme draw_str cannot preserve cross-element
                    // ligatures or contextual forms in complex scripts.
                    canvas.draw_str(
                        &character.source,
                        (character.bounds.left, character.baseline),
                        &font,
                        &paint,
                    );
                }
                canvas.restore();
            }

            canvas.restore();
        }
        self.snapshot_surface(&mut layer, target_width, target_height)
    }
}

fn build_transform_matrix(transform: &Affine2D) -> Matrix {
    Matrix::new_all(
        transform.scale_x as f32,
        transform.skew_x as f32,
        transform.translate_x as f32,
        transform.skew_y as f32,
        transform.scale_y as f32,
        transform.translate_y as f32,
        0.0,
        0.0,
        1.0,
    )
}

impl Renderer for SkiaRenderer {
    fn use_unmanaged_srgba8_surface(&mut self) -> Result<(), LibraryError> {
        self.replace_surface_contract(SkiaSurfaceContract::UnmanagedSrgba8)
    }

    fn use_project_linear_surface(
        &mut self,
        contract: WorkingSurfaceContract,
    ) -> Result<(), LibraryError> {
        self.replace_surface_contract(SkiaSurfaceContract::ProjectLinear(Box::new(contract)))
    }

    fn draw_layer_affine_with_blend(
        &mut self,
        layer: &RenderOutput,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::draw_layer");

        let src_image = match layer {
            RenderOutput::Image(img) => {
                if self.surface_contract.working().is_some() {
                    return Err(LibraryError::Render(
                        "encoded RGBA8 RenderOutput cannot enter a Project linear surface"
                            .to_string(),
                    ));
                }
                image_to_skia(img)?
            }
            RenderOutput::Working(image) => {
                let contract = self.surface_contract.working().ok_or_else(|| {
                    LibraryError::Render(
                        "Project linear RenderOutput cannot enter the unmanaged sRGBA8 surface"
                            .to_string(),
                    )
                })?;
                skia_working_surface::managed_working_to_skia_image(image, contract)?
            }
            RenderOutput::Texture(info) => {
                if self.surface_contract.working().is_some() {
                    return Err(LibraryError::Render(
                        "untyped GPU texture cannot enter a Project linear surface".to_string(),
                    ));
                }
                if let Some(ctx) = self.gpu_context.as_mut() {
                    create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?
                } else {
                    return Err(LibraryError::Render(
                        "Cannot render texture without GPU context".to_string(),
                    ));
                }
            }
        };

        let matrix = build_transform_matrix(transform);
        let identity = *transform == Affine2D::IDENTITY;
        let sampling = if identity {
            SamplingOptions::default()
        } else {
            SamplingOptions::from(CubicResampler::mitchell())
        };
        let blend_runtime = &mut self.blend_runtime;
        let canvas: &Canvas = if let Some(group) = self.group_surfaces.last_mut() {
            group.surface.canvas()
        } else {
            self.surface.canvas()
        };

        with_restored_canvas(canvas, |canvas| {
            canvas.concat(&matrix);
            blend_runtime.draw_image(
                canvas,
                &src_image,
                sampling,
                identity,
                opacity.clamp(0.0, 1.0) as f32,
                blend_mode,
            )
        })?;

        Ok(())
    }

    fn begin_group(
        &mut self,
        width: u32,
        height: u32,
        background_color: &Color,
    ) -> Result<(), LibraryError> {
        let width = width.max(1);
        let height = height.max(1);
        let mut surface = skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &self.surface_contract,
        )?;
        skia_working_surface::clear_authored_color(
            &mut surface,
            &self.surface_contract,
            background_color,
        )?;
        self.group_surfaces.push(GroupSurface {
            width,
            height,
            surface,
        });
        Ok(())
    }

    fn end_group(&mut self) -> Result<RenderOutput, LibraryError> {
        let mut group = self.group_surfaces.pop().ok_or_else(|| {
            LibraryError::Render("end_group called without a matching begin_group".to_string())
        })?;

        // A texture ID is owned by its Surface. Read the isolated target before
        // dropping it so nested groups cannot leave dangling GPU texture IDs.
        self.snapshot_surface(&mut group.surface, group.width, group.height)
    }

    fn apply_masks(
        &mut self,
        layer: &RenderOutput,
        masks: &[crate::model::frame::entity::FrameMask],
    ) -> Result<RenderOutput, LibraryError> {
        if masks.is_empty() {
            return Ok(layer.clone());
        }
        let source = match layer {
            RenderOutput::Image(image) => {
                if self.surface_contract.working().is_some() {
                    return Err(LibraryError::Render(
                        "encoded mask input cannot enter a Project linear surface".to_string(),
                    ));
                }
                image_to_skia(image)?
            }
            RenderOutput::Working(image) => {
                let contract = self.surface_contract.working().ok_or_else(|| {
                    LibraryError::Render(
                        "Project linear mask input cannot enter an unmanaged surface".to_string(),
                    )
                })?;
                skia_working_surface::managed_working_to_skia_image(image, contract)?
            }
            RenderOutput::Texture(_) => {
                return Err(LibraryError::Render(
                    "Timeline masks require an owned raster layer".to_string(),
                ));
            }
        };
        let width = source.width() as u32;
        let height = source.height() as u32;
        let mut output_surface = skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &self.surface_contract,
        )?;
        output_surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        output_surface
            .canvas()
            .draw_image(&source, (0.0, 0.0), Some(&Paint::default()));

        let mut combined = skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &self.surface_contract,
        )?;
        combined.canvas().clear(skia_safe::Color::TRANSPARENT);
        for (index, mask) in masks.iter().enumerate() {
            let mut individual = skia_working_surface::create_surface(
                width,
                height,
                self.gpu_context
                    .as_mut()
                    .map(|context| &mut context.direct_context),
                &self.surface_contract,
            )?;
            let canvas = individual.canvas();
            canvas.clear(if mask.inverted {
                skia_safe::Color::WHITE
            } else {
                skia_safe::Color::TRANSPARENT
            });
            let path = crate::core::rendering::path_geometry::to_skia_path(&mask.path)?;
            let mut path_paint = Paint::default();
            path_paint.set_anti_alias(true);
            path_paint.set_color(skia_safe::Color::WHITE);
            if mask.inverted {
                path_paint.set_blend_mode(skia_safe::BlendMode::DstOut);
            }
            let feather = mask.feather.into_inner() as f32;
            if feather > 0.0 {
                path_paint.set_mask_filter(skia_safe::MaskFilter::blur(
                    skia_safe::BlurStyle::Normal,
                    feather,
                    false,
                ));
            }
            canvas.draw_path(&path, &path_paint);
            let individual_image = individual.image_snapshot();
            let mut combine_paint = Paint::default();
            combine_paint.set_alpha_f(mask.opacity.into_inner() as f32);
            combine_paint.set_blend_mode(match (index, mask.mode) {
                (0, _) | (_, crate::model::authoring::MaskMode::Add) => {
                    skia_safe::BlendMode::SrcOver
                }
                (_, crate::model::authoring::MaskMode::Subtract) => skia_safe::BlendMode::DstOut,
                (_, crate::model::authoring::MaskMode::Intersect) => skia_safe::BlendMode::DstIn,
                (_, crate::model::authoring::MaskMode::Difference) => skia_safe::BlendMode::Xor,
            });
            combined
                .canvas()
                .draw_image(&individual_image, (0.0, 0.0), Some(&combine_paint));
        }
        let mask_image = combined.image_snapshot();
        let mut mask_paint = Paint::default();
        mask_paint.set_blend_mode(skia_safe::BlendMode::DstIn);
        output_surface
            .canvas()
            .draw_image(&mask_image, (0.0, 0.0), Some(&mask_paint));
        self.snapshot_surface(&mut output_surface, width, height)
    }

    fn rasterize_sksl_layer(
        &mut self,
        request: SkSLRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        match request.color_domain {
            SkSLColorDomain::ProjectWorkingLinear if self.surface_contract.working().is_none() => {
                return Err(LibraryError::Render(
                    "Project-working-linear SkSL cannot render into an unmanaged sRGBA8 surface"
                        .to_string(),
                ));
            }
            SkSLColorDomain::ProjectWorkingLinear => {}
        }
        let shader_code = request.shader_code;
        let resolution = request.resolution;
        let time = request.time;
        let transform = request.transform;
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);

            let preprocessed_code = shader_utils::preprocess_shader(shader_code);
            let effect = skia_safe::RuntimeEffect::make_for_shader(&preprocessed_code, None)
                .map_err(|error| {
                    log::error!(
                        "SkSL Compilation Error: {}\nCode:\n{}",
                        error,
                        preprocessed_code
                    );
                    LibraryError::Render(format!("SkSL compilation failed: {error}"))
                })?;
            let uniform_size = effect.uniform_size();
            let mut data: Vec<u8> = vec![0; uniform_size];

            let shader_context = ShaderContext {
                resolution,
                time,
                time_delta: 1.0 / 60.0,
                frame: (time * 60.0).floor(),
                mouse: (0.0, 0.0, 0.0, 0.0),
                date: (2024.0, 1.0, 1.0, 0.0),
            };

            shader_utils::bind_standard_uniforms(&effect, &mut data, &shader_context);

            let uniforms = skia_safe::Data::new_copy(&data);

            let straight_shader =
                effect
                    .make_shader(uniforms, &[], None)
                    .ok_or(LibraryError::Render(
                        "Failed to create SkSL shader".to_string(),
                    ))?;
            // ProjectWorkingLinear is a straight-alpha ABI. Skia assumes every
            // shader result is already premultiplied, so adapt it exactly once
            // without clipping negative or greater-than-one working RGB.
            let shader = self.premultiply_straight_sksl_shader(straight_shader)?;

            let mut paint = Paint::default();
            paint.set_shader(shader);
            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);
            let rect = skia_safe::Rect::from_wh(resolution.0, resolution.1);
            canvas.draw_rect(rect, &paint);
            canvas.restore();
        }

        self.snapshot_surface(&mut layer, target_width, target_height)
    }

    fn rasterize_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={} ensemble={}",
            request.text.len(),
            request.size,
            request.ensemble.is_some()
        ));

        // If ensemble is enabled, use ensemble rendering
        if let Some(ensemble_data) = request.ensemble
            && ensemble_data.enabled
        {
            return self.rasterize_ensemble_text(request, ensemble_data);
        }

        let TextRasterRequest {
            text,
            size,
            font_name,
            styles,
            transform,
            ..
        } = request;

        // Standard text rendering (existing code)
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            let matrix = build_transform_matrix(&transform);
            canvas.save();
            canvas.concat(&matrix);

            for config in styles {
                let style = &config.style;
                let paint =
                    PaintFactory::new(&self.surface_contract).text_paint(style, 1.0, None)?;
                let paragraph = build_text_paragraph(text, font_name, size as f32, Some(&paint));
                paragraph.paint(canvas, (0.0, 0.0));
            }

            canvas.restore();
        }
        self.snapshot_surface(&mut layer, target_width, target_height)
    }

    fn rasterize_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_shape_layer");
        let ShapeRasterRequest {
            path_data,
            canonical_path,
            styles,
            path_effects,
            ensemble,
            transform,
        } = request;
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            let path = super::path_geometry::resolve_renderer_path(canonical_path, path_data)?;
            let matrix = build_transform_matrix(&transform);
            canvas.save();
            canvas.concat(&matrix);
            if let Some(ensemble) = ensemble
                && ensemble.enabled
            {
                legacy_backplate::draw_path_backplates(
                    canvas,
                    &path,
                    &ensemble.decorator_configs,
                    &self.surface_contract,
                )?;
            }
            for config in styles {
                let style = &config.style;
                match style {
                    DrawStyle::Fill { color, offset } => {
                        PaintFactory::new(&self.surface_contract).draw_shape_fill(
                            canvas,
                            &path,
                            color,
                            path_effects,
                            *offset,
                        )?;
                    }
                    DrawStyle::Stroke {
                        color,
                        width,
                        offset,
                        cap,
                        join,
                        miter,
                        dash_array,
                        dash_offset,
                    } => {
                        PaintFactory::new(&self.surface_contract).draw_shape_stroke(
                            canvas,
                            &path,
                            path_effects,
                            StrokeRenderConfig {
                                color,
                                width: *width,
                                offset: *offset,
                                cap,
                                join,
                                miter: *miter,
                                dash_array,
                                dash_offset: *dash_offset,
                            },
                        )?;
                    }
                }
            }
            canvas.restore();
        }
        self.snapshot_surface(&mut layer, target_width, target_height)
    }

    fn read_surface(&mut self, output: &RenderOutput) -> Result<Image, LibraryError> {
        match output {
            RenderOutput::Image(img) => Ok(img.clone()),
            RenderOutput::Working(image) => Err(LibraryError::Render(format!(
                "working image {:?} cannot be read as untyped encoded RGBA8; apply the Project terminal processor",
                image.identity()
            ))),
            RenderOutput::Texture(info) => {
                if let Some(ctx) = self.gpu_context.as_mut() {
                    let image = create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?;
                    // Read pixels
                    let row_bytes = (info.width * 4) as usize;
                    let mut buffer = vec![0u8; (info.height as usize) * row_bytes];
                    let image_info = ImageInfo::new(
                        ISize::new(info.width as i32, info.height as i32),
                        ColorType::RGBA8888,
                        AlphaType::Unpremul,
                        None,
                    );
                    if !image.read_pixels(
                        &image_info,
                        &mut buffer,
                        row_bytes,
                        (0, 0),
                        skia_safe::image::CachingHint::Disallow,
                    ) {
                        return Err(LibraryError::Render(
                            "Failed to read texture pixels".to_string(),
                        ));
                    }
                    Ok(Image::new(info.width, info.height, buffer))
                } else {
                    Err(LibraryError::Render(
                        "No GPU context to read texture".to_string(),
                    ))
                }
            }
        }
    }

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::finalize {}x{}",
            self.width, self.height
        ));

        if !self.group_surfaces.is_empty() {
            return Err(LibraryError::Render(
                "Cannot finalize with unfinished frame groups".to_string(),
            ));
        }

        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();
        }

        // If sharing is enabled, attempt to return a Texture.
        if self.surface_contract.working().is_none()
            && self.sharing_handle.is_some()
            && self.gpu_context.is_some()
            && let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            )
            && let Some(gl_info) = texture.gl_texture_info()
        {
            return Ok(RenderOutput::Texture(TextureInfo {
                texture_id: gl_info.id,
                width: self.width,
                height: self.height,
            }));
        }

        // Fallback to an owned CPU output. Project frames retain working
        // identity here; only RenderService may apply the terminal processor.
        skia_working_surface::snapshot_surface(
            &mut self.surface,
            self.width,
            self.height,
            &self.surface_contract,
        )
    }

    fn clear(&mut self) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::clear");
        self.group_surfaces.clear();
        skia_working_surface::clear_authored_color(
            &mut self.surface,
            &self.surface_contract,
            &self.background_color,
        )
    }

    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        self.gpu_context.as_mut()
    }

    fn set_sharing_context(
        &mut self,
        handle: usize,
        hwnd: Option<isize>,
    ) -> Result<(), LibraryError> {
        if self.sharing_handle == Some(handle) {
            return Ok(());
        }

        log::info!(
            "SkiaRenderer: Setting sharing context handle: {}, hwnd: {:?}",
            handle,
            hwnd
        );
        let mut context = create_gpu_context(Some(handle), hwnd).ok_or_else(|| {
            LibraryError::Render(format!(
                "Cannot create shared GPU context for handle {handle}"
            ))
        })?;
        context.resize(self.width, self.height);
        let (width, height) = (self.width, self.height);
        let surface_contract = self.surface_contract.clone();
        self.replace_render_target(Some(context), Some(handle), hwnd, move |direct_context| {
            skia_working_surface::create_surface(width, height, direct_context, &surface_contract)
                .map_err(|error| {
                    LibraryError::Render(format!(
                        "Cannot create shared Skia surface {width}x{height}: {error}"
                    ))
                })
        })?;
        log::info!("SkiaRenderer: Recreated GPU context with sharing enabled.");
        Ok(())
    }
}

#[cfg(test)]
#[path = "skia_renderer/tests.rs"]
mod tests;
