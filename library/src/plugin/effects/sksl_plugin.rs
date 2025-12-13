use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use crate::plugin::{EffectPlugin, Plugin, PluginCategory, PropertyDefinition, PropertyUiType};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use serde::Deserialize;
use skia_safe::{RuntimeEffect, Data};
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
    pub default: Option<ValueWrapper>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ValueWrapper {
    Float(f64),
    Int(i64),
    Bool(bool),
    String(String),
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
            .map_err(|e| LibraryError::Plugin(format!("Failed to parse TOML: {}", e)))?;

        let result = RuntimeEffect::make_for_shader(sksl_content, None);
        let runtime_effect = match result {
            Ok(effect) => effect,
            Err(error) => return Err(LibraryError::Render(format!("Failed to compile SkSL: {}", error))),
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

    fn category(&self) -> PluginCategory {
         match self.config.category.as_str() {
            "Effect" => PluginCategory::Effect,
             _ => PluginCategory::Effect, // Default to Effect for now
        }
    }

    fn version(&self) -> (u32, u32, u32) {
        self.config.version.unwrap_or((0, 1, 0))
    }
}

impl EffectPlugin for SkslEffectPlugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        use crate::plugin::effects::utils::apply_skia_filter;
        
        use skia_safe::runtime_effect::ChildPtr;
        use skia_safe::{TileMode, SamplingOptions};

        apply_skia_filter(input, gpu_context, |image, _canvas_width, _canvas_height| {
             // Manual uniform packing
             let mut uniform_bytes: Vec<u8> = Vec::new();
             
             for prop in &self.config.properties {
                 if let Some(val) = params.get(&prop.name) {
                     match val {
                         PropertyValue::Number(n) => {
                             let f = n.into_inner() as f32;
                             uniform_bytes.extend_from_slice(&f.to_le_bytes());
                         },
                         PropertyValue::Integer(i) => {
                             let v = *i as i32;
                             uniform_bytes.extend_from_slice(&v.to_le_bytes());
                         },
                         PropertyValue::Boolean(b) => {
                             let v = if *b { 1i32 } else { 0i32 };
                             uniform_bytes.extend_from_slice(&v.to_le_bytes());
                         },
                         _ => {}
                     }
                 } else if let Some(def) = &prop.default {
                     match def {
                         ValueWrapper::Float(f) => {
                             let v = *f as f32;
                             uniform_bytes.extend_from_slice(&v.to_le_bytes());
                         },
                         ValueWrapper::Int(i) => {
                             let v = *i as i32;
                             uniform_bytes.extend_from_slice(&v.to_le_bytes());
                         },
                         ValueWrapper::Bool(b) => {
                             let v = if *b { 1i32 } else { 0i32 };
                             uniform_bytes.extend_from_slice(&v.to_le_bytes());
                         },
                         _ => {}
                     }
                 } else {
                     // Default zero if no value and no default
                     uniform_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                 }
            }

            let data = Data::new_copy(&uniform_bytes);
            
            let input_shader = image.to_shader(
                (TileMode::Clamp, TileMode::Clamp), 
                SamplingOptions::default(), 
                None
            ).ok_or(LibraryError::Render("Failed to create input shader".to_string()))?;
            
            // Runtime shader children: [input_shader]
            // make_shader expects &[ChildPtr]
            let children = [ChildPtr::from(input_shader)]; 

            let shader = self.runtime_effect.make_shader(data, &children, None)
                .ok_or(LibraryError::Render("Failed to create runtime shader".to_string()))?;

            // Create image filter from shader. 
            // Signature guess: shader(shader, crop_rect) (2 args)
            skia_safe::image_filters::shader(shader, None)
                .ok_or(LibraryError::Render("Failed to create shader filter".to_string()))
        })
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        use ordered_float::OrderedFloat;

        self.config.properties.iter().map(|p| {
            let ui_type = match p.r#type.as_str() {
                "Float" => PropertyUiType::Float { 
                    min: p.min.unwrap_or(0.0), 
                    max: p.max.unwrap_or(100.0), 
                    step: p.step.unwrap_or(0.1), 
                    suffix: "".to_string() 
                },
                "Int" => PropertyUiType::Integer { 
                    min: p.min.unwrap_or(0.0) as i64, 
                    max: p.max.unwrap_or(100.0) as i64, 
                    suffix: "".to_string() 
                },
                "Bool" => PropertyUiType::Bool,
                "Color" => PropertyUiType::Color,
                _ => PropertyUiType::Text, // Fallback
            };

            let default_value = match &p.default {
                Some(ValueWrapper::Float(f)) => PropertyValue::Number(OrderedFloat(*f)),
                Some(ValueWrapper::Int(i)) => PropertyValue::Integer(*i),
                Some(ValueWrapper::Bool(b)) => PropertyValue::Boolean(*b),
                Some(ValueWrapper::String(s)) => PropertyValue::String(s.clone()),
                None => PropertyValue::Number(OrderedFloat(0.0)), // Safe default
            };

            PropertyDefinition {
                name: p.name.clone(),
                label: p.label.clone(),
                ui_type,
                default_value,
                category: self.config.category.clone(),
            }
        }).collect()
    }
}
