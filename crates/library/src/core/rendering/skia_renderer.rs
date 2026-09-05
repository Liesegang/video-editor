use crate::cache::SharedCacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::SkSLColorDomain;
use crate::model::frame::runtime_shape::evaluate_text_element_transforms;
use crate::rendering::blend::{BlendRuntime, with_restored_canvas};
use crate::rendering::renderer::{
    Affine2D, ParticleRasterRequest, RenderOutput, Renderer, RetainedRenderLayer,
    ShapeRasterRequest, SkSLRasterRequest, TextRasterRequest, TextureInfo, WorkingSurfaceContract,
};
#[cfg(feature = "gl")]
use crate::rendering::scene_runtime::SceneRuntime;
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

mod context_lifecycle;
mod layer_styles;
mod legacy_backplate;
mod output_compositing;
mod paint;
mod particle;
mod terminal;
#[cfg(feature = "gl")]
mod terminal_compute;
mod vector_layers;

use output_compositing::build_transform_matrix;
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
    retained_group_surfaces: Vec<(RetainedRenderLayer, GroupSurface)>,
    next_retained_layer_id: u64,
    blend_runtime: BlendRuntime,
    sksl_straight_to_premultiplied: Option<skia_safe::RuntimeEffect>,
    #[cfg(feature = "gl")]
    scene_runtime: Option<SceneRuntime>,
    #[cfg(feature = "gl")]
    terminal_compute: Option<terminal_compute::TerminalCompute>,
    gpu_context: Option<GpuContext>,
    require_gpu_surfaces: bool,
    sharing_handle: Option<usize>,
    sharing_hwnd: Option<isize>,
    last_terminal_was_gpu: bool,
}

struct GroupSurface {
    width: u32,
    height: u32,
    surface: Surface,
}

impl SkiaRenderer {
    /// Whether the most recently completed frame used the Project-authorized
    /// GPU terminal stage. Probes must distinguish this from GPU raster alone.
    pub fn last_terminal_was_gpu(&self) -> bool {
        self.last_terminal_was_gpu
    }

    /// Report whether the active root render target is backed by a GPU
    /// texture. Performance probes use this to reject a silent raster-surface
    /// fallback even when a nominal OpenGL context exists.
    pub fn is_gpu_backed(&mut self) -> Result<bool, LibraryError> {
        self.activate_graphics_context()?;
        if self.gpu_context.is_none() {
            return Ok(false);
        }
        Ok(skia_safe::gpu::surfaces::get_backend_texture(
            &mut self.surface,
            skia_safe::surface::BackendHandleAccess::FlushRead,
        )
        .is_some())
    }

    pub fn render_to_texture(&mut self) -> Result<TextureInfo, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::render_to_texture");
        if self.surface_contract.working().is_some() {
            return Err(LibraryError::Render(
                "Project linear surface cannot be exposed as an untyped GPU TextureInfo"
                    .to_string(),
            ));
        }
        self.activate_graphics_context()?;
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
                ctx.ensure_current()?;
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
            false,
        )
        .map_err(|error| {
            LibraryError::Render(format!(
                "Cannot create Skia surface {width}x{height}: {error}"
            ))
        })?;

        #[cfg(feature = "gl")]
        let scene_runtime = gpu_context
            .as_ref()
            .map(|context| SceneRuntime::new(context.create_glow_context()));
        let mut renderer = SkiaRenderer {
            width,
            height,
            background_color,
            surface,
            surface_contract,
            group_surfaces: Vec::new(),
            retained_group_surfaces: Vec::new(),
            next_retained_layer_id: 0,
            blend_runtime: BlendRuntime::new(),
            sksl_straight_to_premultiplied: None,
            #[cfg(feature = "gl")]
            scene_runtime,
            #[cfg(feature = "gl")]
            terminal_compute: gpu_context.as_ref().and_then(|context| {
                terminal_compute::TerminalCompute::new(context.create_glow_context())
            }),
            gpu_context,
            require_gpu_surfaces: false,
            sharing_handle: None,
            sharing_hwnd: None,
            last_terminal_was_gpu: false,
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
        self.activate_graphics_context()?;
        let mut surface = skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &self.surface_contract,
            self.require_gpu_surfaces,
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
        self.retained_group_surfaces.clear();
        Ok(())
    }

    fn create_layer_surface(&mut self) -> Result<Surface, LibraryError> {
        self.activate_graphics_context()?;
        let (width, height) = self.current_target_dimensions();
        skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
            &self.surface_contract,
            self.require_gpu_surfaces,
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
        self.activate_graphics_context()?;
        let mut surface = skia_working_surface::create_surface(
            self.width,
            self.height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &contract,
            self.require_gpu_surfaces,
        )?;
        skia_working_surface::clear_authored_color(
            &mut surface,
            &contract,
            &self.background_color,
        )?;
        self.surface = surface;
        self.surface_contract = contract;
        self.group_surfaces.clear();
        self.retained_group_surfaces.clear();
        Ok(())
    }

    fn current_target_dimensions(&self) -> (u32, u32) {
        self.group_surfaces
            .last()
            .map(|group| (group.width, group.height))
            .unwrap_or((self.width, self.height))
    }

    fn draw_skia_image_affine_with_blend(
        &mut self,
        image: &skia_safe::Image,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
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
                image,
                sampling,
                identity,
                opacity.clamp(0.0, 1.0) as f32,
                blend_mode,
            )
        })
    }
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
        self.activate_graphics_context()?;

        let src_image = self.output_to_skia_image(layer)?;

        self.draw_skia_image_affine_with_blend(&src_image, transform, opacity, blend_mode)
    }

    fn draw_cross_dissolve(
        &mut self,
        from: &RenderOutput,
        to: &RenderOutput,
        progress: f32,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
        self.draw_cross_dissolve_outputs(from, to, progress, blend_mode)
    }

    fn begin_group(
        &mut self,
        width: u32,
        height: u32,
        background_color: &Color,
    ) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
        let width = width.max(1);
        let height = height.max(1);
        let mut surface = skia_working_surface::create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
            &self.surface_contract,
            self.require_gpu_surfaces,
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
        self.activate_graphics_context()?;
        let mut group = self.group_surfaces.pop().ok_or_else(|| {
            LibraryError::Render("end_group called without a matching begin_group".to_string())
        })?;

        // A texture ID is owned by its Surface. Read the isolated target before
        // dropping it so nested groups cannot leave dangling GPU texture IDs.
        self.snapshot_surface(&mut group.surface, group.width, group.height)
    }

    fn end_group_and_draw(
        &mut self,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
        let mut group = self.group_surfaces.pop().ok_or_else(|| {
            LibraryError::Render(
                "end_group_and_draw called without a matching begin_group".to_string(),
            )
        })?;
        let image = group.surface.image_snapshot();
        // Keep `group` alive until the parent draw has retained the image's
        // backend resource. No CPU pixels cross this group boundary.
        let result = self.draw_skia_image_affine_with_blend(&image, transform, opacity, blend_mode);
        drop(image);
        drop(group);
        result
    }

    fn end_group_retained(&mut self) -> Result<RetainedRenderLayer, LibraryError> {
        let group = self.group_surfaces.pop().ok_or_else(|| {
            LibraryError::Render(
                "end_group_retained called without a matching begin_group".to_string(),
            )
        })?;
        let token = RetainedRenderLayer(self.next_retained_layer_id);
        self.next_retained_layer_id =
            self.next_retained_layer_id.checked_add(1).ok_or_else(|| {
                LibraryError::Render("retained render layer identity overflowed".to_string())
            })?;
        self.retained_group_surfaces.push((token, group));
        Ok(token)
    }

    fn release_retained_layer(&mut self, layer: RetainedRenderLayer) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
        let index = self
            .retained_group_surfaces
            .iter()
            .position(|(candidate, _)| *candidate == layer)
            .ok_or_else(|| LibraryError::Render("retained render layer is unavailable".into()))?;
        self.retained_group_surfaces.swap_remove(index);
        Ok(())
    }

    fn draw_cross_dissolve_retained(
        &mut self,
        from: RetainedRenderLayer,
        to: RetainedRenderLayer,
        progress: f32,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
        self.draw_cross_dissolve_retained_layers(from, to, progress, blend_mode)
    }

    fn rasterize_sksl_layer(
        &mut self,
        request: SkSLRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_sksl_layer_surface(request)?;
        self.snapshot_surface(&mut layer, target_width, target_height)
    }

    fn draw_sksl_layer(
        &mut self,
        request: SkSLRasterRequest<'_>,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.create_sksl_layer_surface(request)?;
        self.draw_native_layer_surface(layer, opacity, blend_mode)
    }

    fn rasterize_particle_layer(
        &mut self,
        request: ParticleRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        self.rasterize_particle_output(request)
    }

    fn preflight_particle_backend(
        &mut self,
        target_sizes: &[(u32, u32)],
    ) -> Result<(), LibraryError> {
        self.preflight_particle_output(target_sizes)
    }

    fn draw_particle_layer(
        &mut self,
        request: ParticleRasterRequest<'_>,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        self.draw_particle_output(request, opacity, blend_mode)
    }

    fn rasterize_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_text_layer_surface(request)?;
        self.snapshot_surface(&mut layer, target_width, target_height)
    }

    fn draw_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.create_text_layer_surface(request)?;
        self.draw_native_layer_surface(layer, opacity, blend_mode)
    }

    fn rasterize_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_shape_layer_surface(request)?;
        self.snapshot_surface(&mut layer, target_width, target_height)
    }

    fn draw_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.create_shape_layer_surface(request)?;
        self.draw_native_layer_surface(layer, opacity, blend_mode)
    }

    fn read_surface(&mut self, output: &RenderOutput) -> Result<Image, LibraryError> {
        match output {
            RenderOutput::Image(img) => Ok(img.clone()),
            RenderOutput::Working(image) => Err(LibraryError::Render(format!(
                "working image {:?} cannot be read as untyped encoded RGBA8; apply the Project terminal processor",
                image.identity()
            ))),
            RenderOutput::Texture(info) => {
                self.activate_graphics_context()?;
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
        self.last_terminal_was_gpu = false;
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::finalize {}x{}",
            self.width, self.height
        ));
        self.activate_graphics_context()?;

        self.validate_finalization()?;

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
        self.last_terminal_was_gpu = false;
        let _timer = ScopedTimer::debug("SkiaRenderer::clear");
        self.activate_graphics_context()?;
        self.group_surfaces.clear();
        self.retained_group_surfaces.clear();
        skia_working_surface::clear_authored_color(
            &mut self.surface,
            &self.surface_contract,
            &self.background_color,
        )
    }

    fn finalize_gpu_terminal(
        &mut self,
        chain: &ruvie_color_management::GpuTerminalChain,
    ) -> Result<Option<Image>, LibraryError> {
        self.last_terminal_was_gpu = false;
        let result = self.finalize_terminal_image(chain)?;
        self.last_terminal_was_gpu = result.is_some();
        Ok(result)
    }

    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        let context = self.gpu_context.as_mut()?;
        if let Err(error) = context.ensure_current() {
            log::error!("SkiaRenderer: cannot expose an inactive GPU context: {error}");
            return None;
        }
        Some(context)
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
        let require_gpu_surfaces = self.require_gpu_surfaces;
        self.replace_render_target(Some(context), Some(handle), hwnd, move |direct_context| {
            skia_working_surface::create_surface(
                width,
                height,
                direct_context,
                &surface_contract,
                require_gpu_surfaces,
            )
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
