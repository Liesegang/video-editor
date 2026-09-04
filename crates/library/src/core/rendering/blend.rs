use std::collections::HashMap;

use skia_safe::{
    BlendMode as SkBlendMode, Blender, Canvas, Data, Image as SkImage, Paint, Rect, RuntimeEffect,
    SamplingOptions, Shader, runtime_effect::ChildPtr,
};

use crate::error::LibraryError;
use crate::model::BlendMode;

/// Fixed spatial salt for Dissolve. It deliberately excludes frame time so an
/// unchanged layer never flickers between otherwise identical renders.
const DISSOLVE_SPATIAL_SEED: f32 = 0.754_877_7;

const DISSOLVE_SHADER: &str = r#"
uniform shader source;
uniform float opacity;

half4 main(float2 p) {
    half4 sampled = source.eval(p);
    float coverage = clamp(float(sampled.a) * opacity, 0.0, 1.0);
    float2 cell = floor(p);
    float random = fract(52.9829189 * fract(
        0.06711056 * cell.x + 0.00583715 * cell.y + 0.7548777
    ));
    if (sampled.a <= 0.0 || random >= coverage) {
        return half4(0.0);
    }
    // Runtime shader colors are premultiplied. A selected Dissolve pixel is
    // an opaque sample of the source's straight RGB; alpha becomes coverage.
    return half4(clamp(sampled.rgb / sampled.a, 0.0, 1.0), 1.0);
}
"#;

/// Cached Skia runtime effects owned by one renderer/context.
///
/// Unsupported Photoshop formulas use the W3C source-over blend equation on
/// straight colors. See <https://www.w3.org/TR/compositing-1/#generalformula>.
pub(super) struct BlendRuntime {
    custom_blenders: HashMap<BlendMode, Blender>,
    dissolve_effect: Option<RuntimeEffect>,
}

pub(super) fn with_restored_canvas<T, E>(
    canvas: &Canvas,
    draw: impl FnOnce(&Canvas) -> Result<T, E>,
) -> Result<T, E> {
    canvas.save();
    let result = draw(canvas);
    canvas.restore();
    result
}

impl BlendRuntime {
    pub(super) fn new() -> Self {
        Self {
            custom_blenders: HashMap::new(),
            dissolve_effect: None,
        }
    }

    pub(super) fn configure_paint(
        &mut self,
        paint: &mut Paint,
        mode: BlendMode,
    ) -> Result<(), LibraryError> {
        if let Some(native) = native_blend_mode(mode) {
            paint.set_blend_mode(native);
            return Ok(());
        }
        if mode == BlendMode::Dissolve {
            return Err(LibraryError::Render(
                "Dissolve requires its coordinate-aware source shader".to_string(),
            ));
        }
        let blender = self.custom_blender(mode)?;
        paint.set_blender(blender);
        Ok(())
    }

    pub(super) fn draw_image(
        &mut self,
        canvas: &Canvas,
        image: &SkImage,
        sampling: SamplingOptions,
        identity: bool,
        opacity: f32,
        mode: BlendMode,
    ) -> Result<(), LibraryError> {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        if mode == BlendMode::Dissolve {
            paint.set_shader(self.dissolve_shader(image, sampling, opacity)?);
            paint.set_blend_mode(SkBlendMode::SrcOver);
            canvas.draw_rect(
                Rect::from_wh(image.width() as f32, image.height() as f32),
                &paint,
            );
        } else {
            paint.set_alpha_f(opacity.clamp(0.0, 1.0));
            self.configure_paint(&mut paint, mode)?;
            if identity {
                // Pixel-aligned transient layers already have target resolution.
                canvas.draw_image(image, (0, 0), Some(&paint));
            } else {
                canvas.draw_image_with_sampling_options(image, (0, 0), sampling, Some(&paint));
            }
        }
        Ok(())
    }

    pub(super) fn dissolve_shader(
        &mut self,
        image: &SkImage,
        sampling: SamplingOptions,
        opacity: f32,
    ) -> Result<Shader, LibraryError> {
        if self.dissolve_effect.is_none() {
            let effect =
                RuntimeEffect::make_for_shader(DISSOLVE_SHADER, None).map_err(|error| {
                    LibraryError::Render(format!(
                        "Failed to compile deterministic Dissolve shader: {error}"
                    ))
                })?;
            self.dissolve_effect = Some(effect);
        }
        let effect = self.dissolve_effect.as_ref().ok_or_else(|| {
            LibraryError::Render(
                "Dissolve runtime cache remained empty after compilation".to_string(),
            )
        })?;
        let child = image.to_shader(None, sampling, None).ok_or_else(|| {
            LibraryError::Render("Failed to create Dissolve source image shader".to_string())
        })?;
        let uniforms = Data::new_copy(&opacity.clamp(0.0, 1.0).to_ne_bytes());
        effect
            .make_shader(uniforms, &[ChildPtr::from(child)], None)
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "Failed to instantiate deterministic Dissolve shader (seed {DISSOLVE_SPATIAL_SEED})"
                ))
            })
    }

    fn custom_blender(&mut self, mode: BlendMode) -> Result<Blender, LibraryError> {
        if let Some(blender) = self.custom_blenders.get(&mode) {
            return Ok(blender.clone());
        }
        let formula = custom_formula(mode).ok_or_else(|| {
            LibraryError::Render(format!(
                "Blend mode {} has neither a native nor custom implementation",
                mode.label()
            ))
        })?;
        let source = custom_blender_source(formula);
        let effect = RuntimeEffect::make_for_blender(source, None).map_err(|error| {
            LibraryError::Render(format!(
                "Failed to compile {} blend runtime: {error}",
                mode.label()
            ))
        })?;
        let blender = effect
            .make_blender(Data::new_copy(&[]), None)
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "Failed to instantiate {} blend runtime",
                    mode.label()
                ))
            })?;
        self.custom_blenders.insert(mode, blender.clone());
        Ok(blender)
    }
}

fn native_blend_mode(mode: BlendMode) -> Option<SkBlendMode> {
    Some(match mode {
        BlendMode::Normal => SkBlendMode::SrcOver,
        BlendMode::Behind => SkBlendMode::DstOver,
        // DstOut erases in proportion to actual source coverage, preserving
        // destination pixels beneath transparent source holes.
        BlendMode::Clear => SkBlendMode::DstOut,
        BlendMode::Darken => SkBlendMode::Darken,
        BlendMode::Multiply => SkBlendMode::Multiply,
        BlendMode::ColorBurn => SkBlendMode::ColorBurn,
        BlendMode::Lighten => SkBlendMode::Lighten,
        BlendMode::Screen => SkBlendMode::Screen,
        BlendMode::ColorDodge => SkBlendMode::ColorDodge,
        BlendMode::Overlay => SkBlendMode::Overlay,
        BlendMode::SoftLight => SkBlendMode::SoftLight,
        BlendMode::HardLight => SkBlendMode::HardLight,
        BlendMode::Difference => SkBlendMode::Difference,
        BlendMode::Exclusion => SkBlendMode::Exclusion,
        BlendMode::Hue => SkBlendMode::Hue,
        BlendMode::Saturation => SkBlendMode::Saturation,
        BlendMode::Color => SkBlendMode::Color,
        BlendMode::Luminosity => SkBlendMode::Luminosity,
        BlendMode::Dissolve
        | BlendMode::LinearBurn
        | BlendMode::DarkerColor
        | BlendMode::LinearDodge
        | BlendMode::LighterColor
        | BlendMode::VividLight
        | BlendMode::LinearLight
        | BlendMode::PinLight
        | BlendMode::HardMix
        | BlendMode::Subtract
        | BlendMode::Divide => return None,
    })
}

fn custom_formula(mode: BlendMode) -> Option<&'static str> {
    match mode {
        BlendMode::LinearBurn => Some("max(base + source - 1.0, float3(0.0))"),
        BlendMode::DarkerColor => {
            Some("dot(base, float3(1.0)) <= dot(source, float3(1.0)) ? base : source")
        }
        BlendMode::LinearDodge => Some("min(base + source, float3(1.0))"),
        BlendMode::LighterColor => {
            Some("dot(base, float3(1.0)) >= dot(source, float3(1.0)) ? base : source")
        }
        BlendMode::VividLight => Some(
            "float3(vivid(base.r, source.r), vivid(base.g, source.g), vivid(base.b, source.b))",
        ),
        BlendMode::LinearLight => Some("clamp(base + 2.0 * source - 1.0, 0.0, 1.0)"),
        BlendMode::PinLight => {
            Some("float3(pin(base.r, source.r), pin(base.g, source.g), pin(base.b, source.b))")
        }
        BlendMode::HardMix => Some("step(float3(1.0), base + source)"),
        BlendMode::Subtract => Some("max(base - source, float3(0.0))"),
        // Division by a zero blend channel is defined as white, matching the
        // limiting value as the positive denominator approaches zero.
        BlendMode::Divide => Some(
            "float3(divide_channel(base.r, source.r), divide_channel(base.g, source.g), divide_channel(base.b, source.b))",
        ),
        BlendMode::Normal
        | BlendMode::Dissolve
        | BlendMode::Behind
        | BlendMode::Clear
        | BlendMode::Darken
        | BlendMode::Multiply
        | BlendMode::ColorBurn
        | BlendMode::Lighten
        | BlendMode::Screen
        | BlendMode::ColorDodge
        | BlendMode::Overlay
        | BlendMode::SoftLight
        | BlendMode::HardLight
        | BlendMode::Difference
        | BlendMode::Exclusion
        | BlendMode::Hue
        | BlendMode::Saturation
        | BlendMode::Color
        | BlendMode::Luminosity => None,
    }
}

fn custom_blender_source(formula: &str) -> String {
    format!(
        r#"
float burn(float base, float source) {{
    return source <= 0.0 ? 0.0 : 1.0 - min(1.0, (1.0 - base) / source);
}}

float dodge(float base, float source) {{
    return source >= 1.0 ? 1.0 : min(1.0, base / (1.0 - source));
}}

float vivid(float base, float source) {{
    return source <= 0.5 ? burn(base, 2.0 * source) : dodge(base, 2.0 * source - 1.0);
}}

float pin(float base, float source) {{
    return source <= 0.5 ? min(base, 2.0 * source) : max(base, 2.0 * source - 1.0);
}}

float divide_channel(float base, float source) {{
    return source <= 0.0 ? 1.0 : min(base / source, 1.0);
}}

half4 main(half4 source_pm, half4 base_pm) {{
    float source_alpha = float(source_pm.a);
    float base_alpha = float(base_pm.a);
    float3 source = source_alpha > 0.0 ? float3(source_pm.rgb) / source_alpha : float3(0.0);
    float3 base = base_alpha > 0.0 ? float3(base_pm.rgb) / base_alpha : float3(0.0);
    float3 blended = {formula};
    float3 premul = source_alpha * ((1.0 - base_alpha) * source + base_alpha * blended)
        + (1.0 - source_alpha) * base_alpha * base;
    float output_alpha = source_alpha + base_alpha * (1.0 - source_alpha);
    return half4(half3(premul), half(output_alpha));
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{BlendRuntime, custom_formula, native_blend_mode, with_restored_canvas};
    use crate::model::BlendMode;
    use skia_safe::{Paint, surfaces};

    #[test]
    fn every_non_dissolve_mode_has_exactly_one_paint_implementation()
    -> Result<(), crate::error::LibraryError> {
        let mut runtime = BlendRuntime::new();
        for mode in BlendMode::ALL {
            if mode == BlendMode::Dissolve {
                continue;
            }
            assert_ne!(
                native_blend_mode(mode).is_some(),
                custom_formula(mode).is_some()
            );
            runtime.configure_paint(&mut Paint::default(), mode)?;
        }
        assert_eq!(runtime.custom_blenders.len(), 10);
        Ok(())
    }

    #[test]
    fn saved_canvas_is_restored_before_a_draw_error_propagates() -> Result<(), &'static str> {
        let mut surface =
            surfaces::raster_n32_premul((2, 2)).ok_or("test raster surface unavailable")?;
        let canvas = surface.canvas();
        let initial_save_count = canvas.save_count();
        let result: Result<(), &'static str> = with_restored_canvas(canvas, |canvas| {
            canvas.translate((7.0, 9.0));
            Err("injected draw failure")
        });
        assert_eq!(result, Err("injected draw failure"));
        assert_eq!(canvas.save_count(), initial_save_count);
        Ok(())
    }
}
