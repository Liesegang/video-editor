use crate::error::LibraryError;
use crate::plugin::{EffectPlugin, Plugin};
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
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
    pub properties: Vec<SkslPropertyConfig>,
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
    Float(f64),
    Int(i64),
    Bool(bool),
    String(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
}

#[derive(Clone)]
pub struct SendableRuntimeEffect(RuntimeEffect);

unsafe impl Send for SendableRuntimeEffect {}
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
            .map_err(|e| LibraryError::plugin(format!("Failed to parse TOML: {}", e)))?;

        let result = RuntimeEffect::make_for_shader(sksl_content, None);
        let runtime_effect = match result {
            Ok(effect) => effect,
            Err(error) => {
                return Err(LibraryError::render(format!(
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
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        use crate::builtin::effects::utils::apply_skia_filter;

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
                                let r = c.r as f32 / 255.0;
                                let g = c.g as f32 / 255.0;
                                let b = c.b as f32 / 255.0;
                                let a = c.a as f32 / 255.0;
                                uniform_bytes.extend_from_slice(&r.to_le_bytes());
                                uniform_bytes.extend_from_slice(&g.to_le_bytes());
                                uniform_bytes.extend_from_slice(&b.to_le_bytes());
                                uniform_bytes.extend_from_slice(&a.to_le_bytes());
                            }
                            _ => {
                                log::warn!(
                                    "[WARN] SkSL: Unsupported property value type: {:?}",
                                    val
                                );
                            }
                        }
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
                    .ok_or(LibraryError::render(
                        "Failed to create input shader".to_string(),
                    ))?;

                // Runtime shader children: [input_shader]
                // make_shader expects &[ChildPtr]
                let children = [ChildPtr::from(input_shader)];

                let expected_uniform_size = self.runtime_effect.uniform_size();
                if uniform_bytes.len() != expected_uniform_size {
                    return Err(LibraryError::render(format!(
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
                         LibraryError::render(format!(
                            "Failed to create runtime shader for effect '{}'. Uniform bytes: {}, Expected: {}", 
                            self.config.name, uniform_bytes.len(), expected_uniform_size
                        ))
                    })?;

                // Create image filter from shader.
                // Signature guess: shader(shader, crop_rect) (2 args)
                skia_safe::image_filters::shader(shader, None).ok_or(LibraryError::render(
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
                    "Color" => PropertyUiType::Color,
                    "Vec2" => PropertyUiType::Vec2 {
                        suffix: p.suffix.clone().unwrap_or_default(),
                    },
                    "Vec3" => PropertyUiType::Vec3 {
                        suffix: p.suffix.clone().unwrap_or_default(),
                    },
                    "Vec4" => PropertyUiType::Vec4 {
                        suffix: p.suffix.clone().unwrap_or_default(),
                    },
                    _ => PropertyUiType::Text, // Fallback
                };

                let default_value = match &p.default {
                    Some(ValueWrapper::Float(f)) => PropertyValue::Number(OrderedFloat(*f)),
                    Some(ValueWrapper::Int(i)) => PropertyValue::Integer(*i),
                    Some(ValueWrapper::Bool(b)) => PropertyValue::Boolean(*b),
                    Some(ValueWrapper::String(s)) => PropertyValue::String(s.clone()),
                    Some(ValueWrapper::Vec2(v)) => {
                        PropertyValue::Vec2(crate::project::property::Vec2 {
                            x: OrderedFloat(v[0]),
                            y: OrderedFloat(v[1]),
                        })
                    }
                    Some(ValueWrapper::Vec3(v)) => {
                        if matches!(ui_type, PropertyUiType::Color) {
                            PropertyValue::Color(crate::runtime::color::Color {
                                r: (v[0] * 255.0) as u8,
                                g: (v[1] * 255.0) as u8,
                                b: (v[2] * 255.0) as u8,
                                a: 255,
                            })
                        } else {
                            PropertyValue::Vec3(crate::project::property::Vec3 {
                                x: OrderedFloat(v[0]),
                                y: OrderedFloat(v[1]),
                                z: OrderedFloat(v[2]),
                            })
                        }
                    }
                    Some(ValueWrapper::Vec4(v)) => {
                        if matches!(ui_type, PropertyUiType::Color) {
                            PropertyValue::Color(crate::runtime::color::Color {
                                r: (v[0] * 255.0) as u8,
                                g: (v[1] * 255.0) as u8,
                                b: (v[2] * 255.0) as u8,
                                a: (v[3] * 255.0) as u8,
                            })
                        } else {
                            PropertyValue::Vec4(crate::project::property::Vec4 {
                                x: OrderedFloat(v[0]),
                                y: OrderedFloat(v[1]),
                                z: OrderedFloat(v[2]),
                                w: OrderedFloat(v[3]),
                            })
                        }
                    }
                    None => PropertyValue::Number(OrderedFloat(0.0)), // Safe default
                };

                PropertyDefinition::new(&p.name, ui_type, &p.label, default_value)
            })
            .collect()
    }
}
