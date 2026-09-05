//! RenderOutput boundary conversion and working-domain transition compositing.

use skia_safe::{Canvas, Image as SkImage};

use super::*;

pub(super) fn build_transform_matrix(transform: &Affine2D) -> Matrix {
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

impl SkiaRenderer {
    pub(super) fn snapshot_surface(
        &self,
        surface: &mut Surface,
        width: u32,
        height: u32,
    ) -> Result<RenderOutput, LibraryError> {
        // A transient Surface owns its backend texture ID. Snapshot through
        // the typed boundary before dropping it so ordinary effect layers
        // cannot expose dangling GPU handles. Transition layers use the
        // separate retained-surface path below and stay backend-native.
        skia_working_surface::snapshot_surface(surface, width, height, &self.surface_contract)
    }

    pub(super) fn output_to_skia_image(
        &mut self,
        output: &RenderOutput,
    ) -> Result<SkImage, LibraryError> {
        match output {
            RenderOutput::Image(image) => {
                if self.surface_contract.working().is_some() {
                    return Err(LibraryError::Render(
                        "encoded RGBA8 RenderOutput cannot enter a Project linear surface"
                            .to_string(),
                    ));
                }
                image_to_skia(image)
            }
            RenderOutput::Working(image) => {
                let contract = self.surface_contract.working().ok_or_else(|| {
                    LibraryError::Render(
                        "Project linear RenderOutput cannot enter the unmanaged sRGBA8 surface"
                            .to_string(),
                    )
                })?;
                skia_working_surface::managed_working_to_skia_image(
                    image,
                    contract,
                    self.gpu_context.as_mut(),
                )
            }
            RenderOutput::Texture(info) => {
                if self.surface_contract.working().is_some() {
                    return Err(LibraryError::Render(
                        "untyped GPU texture cannot enter a Project linear surface".to_string(),
                    ));
                }
                let context = self.gpu_context.as_mut().ok_or_else(|| {
                    LibraryError::Render("Cannot render texture without GPU context".to_string())
                })?;
                create_image_from_texture(
                    &mut context.direct_context,
                    info.texture_id,
                    info.width,
                    info.height,
                )
            }
        }
    }

    pub(super) fn draw_cross_dissolve_outputs(
        &mut self,
        from: &RenderOutput,
        to: &RenderOutput,
        progress: f32,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        if !progress.is_finite() {
            return Err(LibraryError::Render(
                "Cross Dissolve progress must be finite".to_string(),
            ));
        }
        let from = self.output_to_skia_image(from)?;
        let to = self.output_to_skia_image(to)?;
        let target_dimensions = self.current_target_dimensions();
        if from.width() != target_dimensions.0 as i32
            || from.height() != target_dimensions.1 as i32
            || to.width() != target_dimensions.0 as i32
            || to.height() != target_dimensions.1 as i32
        {
            return Err(LibraryError::Render(format!(
                "Cross Dissolve sources must match the active target {}x{}",
                target_dimensions.0, target_dimensions.1
            )));
        }
        let blend_runtime = &mut self.blend_runtime;
        let canvas: &Canvas = if let Some(group) = self.group_surfaces.last_mut() {
            group.surface.canvas()
        } else {
            self.surface.canvas()
        };
        with_restored_canvas(canvas, |canvas| {
            blend_runtime.draw_cross_dissolve(canvas, &from, &to, progress, blend_mode)
        })
    }

    pub(super) fn draw_cross_dissolve_retained_layers(
        &mut self,
        from: RetainedRenderLayer,
        to: RetainedRenderLayer,
        progress: f32,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        if !progress.is_finite() {
            return Err(LibraryError::Render(
                "Cross Dissolve progress must be finite".to_string(),
            ));
        }
        if from == to {
            return Err(LibraryError::Render(
                "Cross Dissolve requires two distinct retained layers".to_string(),
            ));
        }
        let take = |layers: &mut Vec<(RetainedRenderLayer, GroupSurface)>, token| {
            layers
                .iter()
                .position(|(candidate, _)| *candidate == token)
                .map(|index| layers.swap_remove(index).1)
        };
        let from_token = from;
        let mut from_group =
            take(&mut self.retained_group_surfaces, from_token).ok_or_else(|| {
                LibraryError::Render("Cross Dissolve from layer is unavailable".to_string())
            })?;
        let mut to_group = match take(&mut self.retained_group_surfaces, to) {
            Some(to) => to,
            None => {
                // Restore ownership so a failed lookup does not leak or lose
                // the valid source token.
                self.retained_group_surfaces.push((from_token, from_group));
                return Err(LibraryError::Render(
                    "Cross Dissolve to layer is unavailable".to_string(),
                ));
            }
        };
        let target_dimensions = self.current_target_dimensions();
        if (from_group.width, from_group.height) != target_dimensions
            || (to_group.width, to_group.height) != target_dimensions
        {
            return Err(LibraryError::Render(format!(
                "Cross Dissolve retained sources must match the active target {}x{}",
                target_dimensions.0, target_dimensions.1
            )));
        }
        let from_image = from_group.surface.image_snapshot();
        let to_image = to_group.surface.image_snapshot();
        let blend_runtime = &mut self.blend_runtime;
        let canvas: &Canvas = if let Some(group) = self.group_surfaces.last_mut() {
            group.surface.canvas()
        } else {
            self.surface.canvas()
        };
        with_restored_canvas(canvas, |canvas| {
            blend_runtime.draw_cross_dissolve(canvas, &from_image, &to_image, progress, blend_mode)
        })
    }
}
