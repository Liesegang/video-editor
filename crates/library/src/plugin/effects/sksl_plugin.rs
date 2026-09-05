use crate::error::LibraryError;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use crate::plugin::{EffectColorDomain, EffectPlugin, Plugin};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use serde::Deserialize;
use skia_safe::{Data, RuntimeEffect};
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone)]
pub struct SkslPluginConfig {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version: Option<(u32, u32, u32)>,
    #[serde(default)]
    pub color_domain: SkslEffectColorDomain,
    pub properties: Vec<SkslPropertyConfig>,
}

/// Pixel contract authored by one SkSL effect.
///
/// Existing third-party shaders remain on the historical encoded-sRGBA8
/// boundary unless they explicitly opt in. Project-linear shaders receive and
/// return premultiplied RGBAF32 through Skia without a display transform or an
/// 8-bit compatibility roundtrip.
#[derive(Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkslEffectColorDomain {
    #[default]
    UnmanagedSrgba8,
    ProjectLinear,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SkslPropertyConfig {
    pub name: String,
    pub label: String,
    pub r#type: String, // "Float", "Int", "Color", etc.
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub suffix: Option<String>,
    pub default: Option<ValueWrapper>,
    pub min_hard_limit: Option<bool>,
    pub max_hard_limit: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ValueWrapper {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
}

#[derive(Clone)]
struct SendableRuntimeEffect(RuntimeEffect);

#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "skia-safe does not mark immutable SkRuntimeEffect Send even though Skia permits cross-thread shared use"
)]
// SAFETY: SkRuntimeEffect is immutable after construction and uses Skia's
// thread-safe intrusive reference count. This wrapper exposes only shared
// dereferencing; shader instances are created per apply call.
unsafe impl Send for SendableRuntimeEffect {}
// SAFETY: The wrapped runtime effect is immutable and all exposed operations
// take &self. Mutable GPU state is supplied separately to each apply call.
unsafe impl Sync for SendableRuntimeEffect {}

impl std::ops::Deref for SendableRuntimeEffect {
    type Target = RuntimeEffect;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct SkslEffectPlugin {
    config: SkslPluginConfig,
    runtime_effect: SendableRuntimeEffect,
    id_static: &'static str,
}

impl SkslEffectPlugin {
    pub fn new(toml_content: &str, sksl_content: &str) -> Result<Self, LibraryError> {
        let config: SkslPluginConfig = toml::from_str(toml_content)
            .map_err(|e| LibraryError::Plugin(format!("Failed to parse TOML: {}", e)))?;

        let result = RuntimeEffect::make_for_shader(sksl_content, None);
        let runtime_effect = match result {
            Ok(effect) => effect,
            Err(error) => {
                return Err(LibraryError::Render(format!(
                    "Failed to compile SkSL: {}",
                    error
                )));
            }
        };

        // Leak the ID to satisfy &'static str requirement
        let id_static = Box::leak(config.id.clone().into_boxed_str());

        Ok(Self {
            config,
            runtime_effect: SendableRuntimeEffect(runtime_effect),
            id_static,
        })
    }
}

impl Plugin for SkslEffectPlugin {
    fn id(&self) -> &'static str {
        self.id_static
    }

    fn name(&self) -> String {
        self.config.name.clone()
    }

    fn category(&self) -> String {
        self.config.category.clone()
    }

    fn version(&self) -> (u32, u32, u32) {
        self.config.version.unwrap_or((0, 1, 0))
    }

    fn impl_type(&self) -> String {
        "SkSL".to_string()
    }
}

impl EffectPlugin for SkslEffectPlugin {
    fn color_domain(&self) -> EffectColorDomain {
        match self.config.color_domain {
            SkslEffectColorDomain::UnmanagedSrgba8 => EffectColorDomain::UnmanagedSrgba8Only,
            SkslEffectColorDomain::ProjectLinear => EffectColorDomain::ProjectLinearPreserving,
        }
    }

    fn project_linear_color_parameters(&self) -> Vec<&str> {
        if self.config.color_domain != SkslEffectColorDomain::ProjectLinear {
            return Vec::new();
        }
        self.config
            .properties
            .iter()
            .filter(|property| property.r#type == "Color")
            .map(|property| property.name.as_str())
            .collect()
    }

    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        use crate::plugin::effects::utils::apply_skia_filter;

        use skia_safe::runtime_effect::ChildPtr;
        use skia_safe::{SamplingOptions, TileMode};

        apply_skia_filter(
            input,
            gpu_context,
            |image, _canvas_width, _canvas_height| {
                // Manual uniform packing
                let mut uniform_bytes: Vec<u8> = Vec::new();

                for prop in &self.config.properties {
                    if prop.name == "u_resolution" {
                        // Auto-inject resolution
                        let w = _canvas_width as f32;
                        let h = _canvas_height as f32;
                        uniform_bytes.extend_from_slice(&w.to_le_bytes());
                        uniform_bytes.extend_from_slice(&h.to_le_bytes());
                        continue;
                    }

                    if let Some(val) = params.get(&prop.name) {
                        match val {
                            PropertyValue::Number(n) => {
                                let f = n.into_inner() as f32;
                                uniform_bytes.extend_from_slice(&f.to_le_bytes());
                            }
                            PropertyValue::Integer(i) => {
                                let v = *i as i32;
                                uniform_bytes.extend_from_slice(&v.to_le_bytes());
                            }
                            PropertyValue::Boolean(b) => {
                                let v = if *b { 1i32 } else { 0i32 };
                                uniform_bytes.extend_from_slice(&v.to_le_bytes());
                            }
                            PropertyValue::Vec2(v) => {
                                let x = v.x.into_inner() as f32;
                                let y = v.y.into_inner() as f32;
                                uniform_bytes.extend_from_slice(&x.to_le_bytes());
                                uniform_bytes.extend_from_slice(&y.to_le_bytes());
                            }
                            PropertyValue::Vec3(v) => {
                                let x = v.x.into_inner() as f32;
                                let y = v.y.into_inner() as f32;
                                let z = v.z.into_inner() as f32;
                                uniform_bytes.extend_from_slice(&x.to_le_bytes());
                                uniform_bytes.extend_from_slice(&y.to_le_bytes());
                                uniform_bytes.extend_from_slice(&z.to_le_bytes());
                            }
                            PropertyValue::Vec4(v) => {
                                let x = v.x.into_inner() as f32;
                                let y = v.y.into_inner() as f32;
                                let z = v.z.into_inner() as f32;
                                let w = v.w.into_inner() as f32;
                                uniform_bytes.extend_from_slice(&x.to_le_bytes());
                                uniform_bytes.extend_from_slice(&y.to_le_bytes());
                                uniform_bytes.extend_from_slice(&z.to_le_bytes());
                                uniform_bytes.extend_from_slice(&w.to_le_bytes());
                            }
                            PropertyValue::Color(c) => {
                                if matches!(input, RenderOutput::Working(_))
                                    && self.config.color_domain
                                        == SkslEffectColorDomain::ProjectLinear
                                {
                                    return Err(LibraryError::Render(format!(
                                        "Project-linear SkSL effect '{}' received encoded sRGBA8 property '{}'; RenderService must convert authored colors into the exact Project working space",
                                        self.config.name, prop.name
                                    )));
                                }
                                let r = c.r as f32 / 255.0;
                                let g = c.g as f32 / 255.0;
                                let b = c.b as f32 / 255.0;
                                let a = c.a as f32 / 255.0;
                                uniform_bytes.extend_from_slice(&r.to_le_bytes());
                                uniform_bytes.extend_from_slice(&g.to_le_bytes());
                                uniform_bytes.extend_from_slice(&b.to_le_bytes());
                                uniform_bytes.extend_from_slice(&a.to_le_bytes());
                            }
                            PropertyValue::ColorValue(c) => {
                                let RenderOutput::Working(working) = input else {
                                    return Err(LibraryError::Render(format!(
                                        "SkSL effect '{}' received a typed color outside the Project-linear render boundary",
                                        self.config.name
                                    )));
                                };
                                if self.config.color_domain != SkslEffectColorDomain::ProjectLinear
                                {
                                    return Err(LibraryError::Render(format!(
                                        "SkSL effect '{}' did not declare the Project-linear color domain",
                                        self.config.name
                                    )));
                                }
                                let expected = working.identity().working_space();
                                if c.color_space().as_str() != expected {
                                    return Err(LibraryError::Render(format!(
                                        "Project-linear SkSL effect '{}' property '{}' is in color space '{}', expected exact Project working space '{}'",
                                        self.config.name,
                                        prop.name,
                                        c.color_space(),
                                        expected
                                    )));
                                }
                                for component in c.rgba() {
                                    let component = component as f32;
                                    if !component.is_finite() {
                                        return Err(LibraryError::Render(format!(
                                            "Project-linear SkSL effect '{}' property '{}' exceeds RGBAF32 range",
                                            self.config.name, prop.name
                                        )));
                                    }
                                    uniform_bytes.extend_from_slice(&component.to_le_bytes());
                                }
                            }
                            _ => {
                                log::warn!(
                                    "[WARN] SkSL: Unsupported property value type: {:?}",
                                    val
                                );
                            }
                        }
                    } else if prop.r#type == "Color"
                        && matches!(input, RenderOutput::Working(_))
                        && self.config.color_domain == SkslEffectColorDomain::ProjectLinear
                    {
                        return Err(LibraryError::Render(format!(
                            "Project-linear SkSL effect '{}' is missing converted working-space color property '{}'",
                            self.config.name, prop.name
                        )));
                    } else if let Some(def) = &prop.default {
                        match def {
                            ValueWrapper::Float(f) => {
                                let v = *f as f32;
                                uniform_bytes.extend_from_slice(&v.to_le_bytes());
                            }
                            ValueWrapper::Int(i) => {
                                let v = *i as i32;
                                uniform_bytes.extend_from_slice(&v.to_le_bytes());
                            }
                            ValueWrapper::Bool(b) => {
                                let v = if *b { 1i32 } else { 0i32 };
                                uniform_bytes.extend_from_slice(&v.to_le_bytes());
                            }
                            ValueWrapper::Vec2(v) => {
                                let x = v[0] as f32;
                                let y = v[1] as f32;
                                uniform_bytes.extend_from_slice(&x.to_le_bytes());
                                uniform_bytes.extend_from_slice(&y.to_le_bytes());
                            }
                            ValueWrapper::Vec3(v) => {
                                let x = v[0] as f32;
                                let y = v[1] as f32;
                                let z = v[2] as f32;
                                uniform_bytes.extend_from_slice(&x.to_le_bytes());
                                uniform_bytes.extend_from_slice(&y.to_le_bytes());
                                uniform_bytes.extend_from_slice(&z.to_le_bytes());
                            }
                            ValueWrapper::Vec4(v) => {
                                let x = v[0] as f32;
                                let y = v[1] as f32;
                                let z = v[2] as f32;
                                let w = v[3] as f32;
                                uniform_bytes.extend_from_slice(&x.to_le_bytes());
                                uniform_bytes.extend_from_slice(&y.to_le_bytes());
                                uniform_bytes.extend_from_slice(&z.to_le_bytes());
                                uniform_bytes.extend_from_slice(&w.to_le_bytes());
                            }
                            _ => {
                                log::warn!(
                                    "[WARN] SkSL: Unsupported default value type: {:?}",
                                    def
                                );
                            }
                        }
                    } else {
                        // Default zero if no value and no default.
                        // For Vec2 we need 2 floats? Handle basic types first.
                        // If type is Vec2, we should push 2 zeros.
                        if prop.r#type == "Vec2" {
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                        } else if prop.r#type == "Vec3" {
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                        } else if prop.r#type == "Vec4" {
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                        } else {
                            uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                        }
                    }
                }

                let data = Data::new_copy(&uniform_bytes);

                let input_shader = image
                    .to_shader(
                        (TileMode::Clamp, TileMode::Clamp),
                        SamplingOptions::default(),
                        None,
                    )
                    .ok_or(LibraryError::Render(
                        "Failed to create input shader".to_string(),
                    ))?;

                // Runtime shader children: [input_shader]
                // make_shader expects &[ChildPtr]
                let children = [ChildPtr::from(input_shader)];

                let expected_uniform_size = self.runtime_effect.uniform_size();
                if uniform_bytes.len() != expected_uniform_size {
                    return Err(LibraryError::Render(format!(
                        "Uniform size mismatch for effect '{}': expected {} bytes, got {} bytes",
                        self.config.name,
                        expected_uniform_size,
                        uniform_bytes.len()
                    )));
                }

                let shader = self
                    .runtime_effect
                    .make_shader(data, &children, None)
                    .ok_or_else(|| {
                         LibraryError::Render(format!(
                            "Failed to create runtime shader for effect '{}'. Uniform bytes: {}, Expected: {}",
                            self.config.name, uniform_bytes.len(), expected_uniform_size
                        ))
                    })?;

                // Create image filter from shader.
                // Signature guess: shader(shader, crop_rect) (2 args)
                skia_safe::image_filters::shader(shader, None).ok_or(LibraryError::Render(
                    "Failed to create shader filter".to_string(),
                ))
            },
        )
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        use ordered_float::OrderedFloat;

        self.config
            .properties
            .iter()
            .filter(|p| p.name != "u_resolution" && p.name != "u_time")
            .map(|p| {
                let ui_type = match p.r#type.as_str() {
                    "Float" => PropertyUiType::Float {
                        min: p.min.unwrap_or(0.0),
                        max: p.max.unwrap_or(100.0),
                        step: p.step.unwrap_or(0.1),
                        suffix: "".to_string(),
                        min_hard_limit: p.min_hard_limit.unwrap_or(false),
                        max_hard_limit: p.max_hard_limit.unwrap_or(false),
                    },
                    "Int" => PropertyUiType::Integer {
                        min: p.min.unwrap_or(0.0) as i64,
                        max: p.max.unwrap_or(100.0) as i64,
                        suffix: "".to_string(),
                        min_hard_limit: p.min_hard_limit.unwrap_or(false),
                        max_hard_limit: p.max_hard_limit.unwrap_or(false),
                    },
                    "Bool" => PropertyUiType::Bool,
                    "Color" if self.config.color_domain == SkslEffectColorDomain::ProjectLinear => {
                        PropertyUiType::ColorValue
                    }
                    "Color" => PropertyUiType::Color,
                    "Vec2" => PropertyUiType::vec2_with_range(
                        p.min.unwrap_or(-1_000_000.0),
                        p.max.unwrap_or(1_000_000.0),
                        p.step.unwrap_or(0.1),
                        p.suffix.clone().unwrap_or_default(),
                        p.min_hard_limit.unwrap_or(false),
                        p.max_hard_limit.unwrap_or(false),
                    ),
                    "Vec3" => PropertyUiType::vec3_with_range(
                        p.min.unwrap_or(-1_000_000.0),
                        p.max.unwrap_or(1_000_000.0),
                        p.step.unwrap_or(0.1),
                        p.suffix.clone().unwrap_or_default(),
                        p.min_hard_limit.unwrap_or(false),
                        p.max_hard_limit.unwrap_or(false),
                    ),
                    "Vec4" => PropertyUiType::vec4_with_range(
                        p.min.unwrap_or(-1_000_000.0),
                        p.max.unwrap_or(1_000_000.0),
                        p.step.unwrap_or(0.1),
                        p.suffix.clone().unwrap_or_default(),
                        p.min_hard_limit.unwrap_or(false),
                        p.max_hard_limit.unwrap_or(false),
                    ),
                    _ => PropertyUiType::Text, // Fallback
                };

                let default_value = match &p.default {
                    Some(ValueWrapper::Float(value))
                        if matches!(ui_type, PropertyUiType::Integer { .. })
                            && value.is_finite()
                            && value.fract() == 0.0
                            && *value >= i64::MIN as f64
                            && *value <= i64::MAX as f64 =>
                    {
                        PropertyValue::Integer(*value as i64)
                    }
                    Some(ValueWrapper::Float(value)) => PropertyValue::Number(OrderedFloat(*value)),
                    Some(ValueWrapper::Int(value))
                        if matches!(ui_type, PropertyUiType::Float { .. }) =>
                    {
                        PropertyValue::Number(OrderedFloat(*value as f64))
                    }
                    Some(ValueWrapper::Int(value)) => PropertyValue::Integer(*value),
                    Some(ValueWrapper::Bool(b)) => PropertyValue::Boolean(*b),
                    Some(ValueWrapper::String(s)) => PropertyValue::String(s.clone()),
                    Some(ValueWrapper::Vec2(v)) => {
                        PropertyValue::Vec2(crate::model::property::Vec2 {
                            x: OrderedFloat(v[0]),
                            y: OrderedFloat(v[1]),
                        })
                    }
                    Some(ValueWrapper::Vec3(v)) => {
                        if matches!(ui_type, PropertyUiType::Color | PropertyUiType::ColorValue) {
                            let color = crate::model::frame::color::Color {
                                r: (v[0] * 255.0) as u8,
                                g: (v[1] * 255.0) as u8,
                                b: (v[2] * 255.0) as u8,
                                a: 255,
                            };
                            if matches!(ui_type, PropertyUiType::ColorValue) {
                                PropertyValue::ColorValue(
                                    crate::model::property::ColorValue::from_straight_srgba8(
                                        &color,
                                    ),
                                )
                            } else {
                                PropertyValue::Color(color)
                            }
                        } else {
                            PropertyValue::Vec3(crate::model::property::Vec3 {
                                x: OrderedFloat(v[0]),
                                y: OrderedFloat(v[1]),
                                z: OrderedFloat(v[2]),
                            })
                        }
                    }
                    Some(ValueWrapper::Vec4(v)) => {
                        if matches!(ui_type, PropertyUiType::Color | PropertyUiType::ColorValue) {
                            let color = crate::model::frame::color::Color {
                                r: (v[0] * 255.0) as u8,
                                g: (v[1] * 255.0) as u8,
                                b: (v[2] * 255.0) as u8,
                                a: (v[3] * 255.0) as u8,
                            };
                            if matches!(ui_type, PropertyUiType::ColorValue) {
                                PropertyValue::ColorValue(
                                    crate::model::property::ColorValue::from_straight_srgba8(
                                        &color,
                                    ),
                                )
                            } else {
                                PropertyValue::Color(color)
                            }
                        } else {
                            PropertyValue::Vec4(crate::model::property::Vec4 {
                                x: OrderedFloat(v[0]),
                                y: OrderedFloat(v[1]),
                                z: OrderedFloat(v[2]),
                                w: OrderedFloat(v[3]),
                            })
                        }
                    }
                    None => match &ui_type {
                        PropertyUiType::Float { .. } => PropertyValue::Number(OrderedFloat(0.0)),
                        PropertyUiType::Integer { .. } => PropertyValue::Integer(0),
                        PropertyUiType::Bool => PropertyValue::Boolean(false),
                        PropertyUiType::Color => {
                            PropertyValue::Color(crate::model::frame::color::Color {
                                r: 0,
                                g: 0,
                                b: 0,
                                a: 255,
                            })
                        }
                        PropertyUiType::ColorValue => PropertyValue::ColorValue(
                            crate::model::property::ColorValue::from_straight_srgba8(
                                &crate::model::frame::color::Color {
                                    r: 0,
                                    g: 0,
                                    b: 0,
                                    a: 255,
                                },
                            ),
                        ),
                        PropertyUiType::Path => {
                            PropertyValue::Path(crate::model::path::PathValue::empty(
                                crate::model::path::FillRule::NonZero,
                            ))
                        }
                        PropertyUiType::Vec2 { .. } => {
                            PropertyValue::Vec2(crate::model::property::Vec2 {
                                x: OrderedFloat(0.0),
                                y: OrderedFloat(0.0),
                            })
                        }
                        PropertyUiType::Vec3 { .. } => {
                            PropertyValue::Vec3(crate::model::property::Vec3 {
                                x: OrderedFloat(0.0),
                                y: OrderedFloat(0.0),
                                z: OrderedFloat(0.0),
                            })
                        }
                        PropertyUiType::Vec4 { .. } => {
                            PropertyValue::Vec4(crate::model::property::Vec4 {
                                x: OrderedFloat(0.0),
                                y: OrderedFloat(0.0),
                                z: OrderedFloat(0.0),
                                w: OrderedFloat(0.0),
                            })
                        }
                        PropertyUiType::Text
                        | PropertyUiType::MultilineText
                        | PropertyUiType::Font
                        | PropertyUiType::Dropdown { .. }
                        | PropertyUiType::Gradient
                        | PropertyUiType::Pattern => PropertyValue::String(String::new()),
                    },
                };

                PropertyDefinition::new(&p.name, ui_type, &p.label, default_value)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::{ColorSpaceRef, ColorValue};
    use crate::plugin::PluginManager;
    use ruvie_color_management::{
        BuiltinColorTransform, ColorContext, ColorTransformBackend, LINEAR_SRGB_SPACE_ID,
        LinearWorkingImage, ManagedLinearWorkingImage, WorkingColorIdentity,
    };
    use std::sync::Arc;

    const SILHOUETTE_CONFIG: &str =
        include_str!("../../../../../assets/plugins/sksl/silhouette/config.toml");
    const SILHOUETTE_SHADER: &str =
        include_str!("../../../../../assets/plugins/sksl/silhouette/shader.sksl");
    const MOSAIC_CONFIG: &str =
        include_str!("../../../../../assets/plugins/sksl/mosaic/config.toml");
    const MOSAIC_SHADER: &str =
        include_str!("../../../../../assets/plugins/sksl/mosaic/shader.sksl");

    fn managed_pixel(pixel: [f32; 4]) -> RenderOutput {
        let backend = BuiltinColorTransform;
        let working = backend
            .verify_working_space(LINEAR_SRGB_SPACE_ID, &ColorContext::default())
            .expect("verify test working space");
        let identity =
            WorkingColorIdentity::from_verified("silhouette-linear-test", working).unwrap();
        let pixels = LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![pixel]).unwrap();
        // SAFETY: The fixture pixel is authored directly in the verified
        // linear-sRGB test identity and uses premultiplied alpha.
        RenderOutput::Working(unsafe {
            ManagedLinearWorkingImage::from_working_pixels_unchecked(identity, pixels)
        })
    }

    fn working_color(rgba: [f64; 4]) -> PropertyValue {
        PropertyValue::ColorValue(
            ColorValue::new(ColorSpaceRef::linear_srgb(), rgba).expect("valid working test color"),
        )
    }

    fn assert_pixel_near(actual: [f32; 4], expected: [f32; 4]) {
        for (component, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
            assert!(
                (actual - expected).abs() <= 2.0e-3,
                "component {component}: expected {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn silhouette_keeps_working_identity_and_applies_straight_color_as_premultiplied() {
        let manager = PluginManager::new();
        let plugin = SkslEffectPlugin::new(SILHOUETTE_CONFIG, SILHOUETTE_SHADER).unwrap();
        assert_eq!(
            plugin.color_domain(),
            EffectColorDomain::ProjectLinearPreserving
        );
        assert_eq!(plugin.project_linear_color_parameters(), ["color"]);
        manager.register_effect(Arc::new(plugin));

        let input = managed_pixel([-0.25, 1.5, 0.75, 0.5]);
        let RenderOutput::Working(input_working) = &input else {
            panic!("managed_pixel must return a working image");
        };
        let expected_identity = input_working.identity().clone();
        let params = HashMap::from([("color".to_string(), working_color([0.25, 0.5, 2.0, 0.5]))]);
        let output = manager
            .apply_effect("silhouette", &input, &params, None)
            .expect("linear silhouette should process RGBAF32 directly");
        let RenderOutput::Working(output) = output else {
            panic!("silhouette dropped the managed working contract");
        };
        assert_eq!(output.identity(), &expected_identity);
        assert_pixel_near(output.pixels().pixels()[0], [0.0625, 0.125, 0.5, 0.25]);
    }

    #[test]
    fn silhouette_transparent_input_has_no_hidden_rgb() {
        let manager = PluginManager::new();
        manager.register_effect(Arc::new(
            SkslEffectPlugin::new(SILHOUETTE_CONFIG, SILHOUETTE_SHADER).unwrap(),
        ));
        let params = HashMap::from([("color".to_string(), working_color([3.0, -1.0, 2.0, 1.0]))]);
        let output = manager
            .apply_effect("silhouette", &managed_pixel([0.0; 4]), &params, None)
            .unwrap();
        let RenderOutput::Working(output) = output else {
            panic!("silhouette dropped the managed working contract");
        };
        assert_eq!(output.pixels().pixels()[0], [0.0; 4]);
    }

    #[test]
    fn sksl_effects_remain_legacy_only_without_explicit_linear_opt_in() {
        let config = r#"
id = "legacy-test"
name = "Legacy Test"
category = "Test"
properties = []
"#;
        let shader = "half4 main(float2 p) { return half4(0); }";
        let plugin = SkslEffectPlugin::new(config, shader).unwrap();
        assert_eq!(
            plugin.color_domain(),
            EffectColorDomain::UnmanagedSrgba8Only
        );
        assert!(plugin.project_linear_color_parameters().is_empty());
    }

    #[test]
    fn mosaic_preserves_the_project_linear_frame_contract() {
        let manager = PluginManager::new();
        let plugin = SkslEffectPlugin::new(MOSAIC_CONFIG, MOSAIC_SHADER).unwrap();
        assert_eq!(
            plugin.color_domain(),
            EffectColorDomain::ProjectLinearPreserving
        );
        manager.register_effect(Arc::new(plugin));

        let input = managed_pixel([0.25, 0.5, 1.5, 1.0]);
        let RenderOutput::Working(input_working) = &input else {
            panic!("managed_pixel must return a working image");
        };
        let expected_identity = input_working.identity().clone();
        let output = manager
            .apply_effect(
                "mosaic",
                &input,
                &HashMap::from([("pixel_size".to_string(), PropertyValue::from(1.0))]),
                None,
            )
            .expect("Mosaic should run directly on Project-linear RGBAF32");
        let RenderOutput::Working(output) = output else {
            panic!("Mosaic dropped the managed working contract");
        };
        assert_eq!(output.identity(), &expected_identity);
        assert_pixel_near(output.pixels().pixels()[0], [0.25, 0.5, 1.5, 1.0]);
    }
}

#[cfg(test)]
#[path = "sksl_plugin/bundled_tests.rs"]
mod bundled_tests;
