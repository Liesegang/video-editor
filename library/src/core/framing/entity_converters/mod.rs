//! Entity converters for transforming TrackClip entities into FrameObjects.
//!
//! This module is organized into submodules per converter type:
//! - `context`: Shared FrameEvaluationContext
//! - `video`: VideoEntityConverter
//! - `image`: ImageEntityConverter  
//! - `text`: TextEntityConverter
//! - `shape`: ShapeEntityConverter
//! - `sksl`: SkSLEntityConverter

mod context;
mod image;
mod shape;
mod sksl;
mod text;
mod video;

use log::warn;
use std::collections::HashMap;
use std::sync::Arc;

use crate::model::frame::entity::FrameObject;
use crate::model::project::TrackClip;

// Re-export from submodules
pub use context::FrameEvaluationContext;
pub use image::ImageEntityConverter;
pub use shape::ShapeEntityConverter;
pub use sksl::SkSLEntityConverter;
pub use text::TextEntityConverter;
pub use video::VideoEntityConverter;

/// Trait for converting an Entity into a FrameObject.
pub trait EntityConverter: Send + Sync {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject>;

    fn get_bounds(
        &self,
        _evaluator: &FrameEvaluationContext,
        _track_clip: &TrackClip,
        _frame_number: u64,
    ) -> Option<(f32, f32, f32, f32)> {
        None
    }
}

/// Trait for entity converter plugins.
pub trait EntityConverterPlugin: crate::plugin::Plugin {
    fn register_converters(&self, registry: &mut EntityConverterRegistry);

    fn plugin_type(&self) -> crate::plugin::PluginCategory {
        crate::plugin::PluginCategory::EntityConverter
    }
}

/// Registry for EntityConverter implementations.
#[derive(Clone)]
pub struct EntityConverterRegistry {
    converters: HashMap<String, Arc<dyn EntityConverter>>,
}

impl EntityConverterRegistry {
    pub fn new() -> Self {
        Self {
            converters: HashMap::new(),
        }
    }

    pub fn register(&mut self, entity_type: String, converter: Arc<dyn EntityConverter>) {
        self.converters.insert(entity_type, converter);
    }

    pub fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let kind_str = track_clip.kind.to_string();
        match self.converters.get(&kind_str) {
            Some(converter) => converter.convert_entity(evaluator, track_clip, frame_number),
            None => {
                warn!("No converter registered for entity type '{}'", kind_str);
                None
            }
        }
    }

    pub fn get_entity_bounds(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<(f32, f32, f32, f32)> {
        let kind_str = track_clip.kind.to_string();
        match self.converters.get(&kind_str) {
            Some(converter) => converter.get_bounds(evaluator, track_clip, frame_number),
            None => None,
        }
    }
}

pub struct BuiltinEntityConverterPlugin;

impl BuiltinEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for BuiltinEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "builtin_entity_converters"
    }

    fn name(&self) -> String {
        "Builtin Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for BuiltinEntityConverterPlugin {
    fn register_converters(&self, registry: &mut EntityConverterRegistry) {
        register_builtin_entity_converters(registry);
    }
}

pub fn register_builtin_entity_converters(registry: &mut EntityConverterRegistry) {
    registry.register("video".to_string(), Arc::new(VideoEntityConverter));
    registry.register("image".to_string(), Arc::new(ImageEntityConverter));
    registry.register("text".to_string(), Arc::new(TextEntityConverter));
    registry.register("shape".to_string(), Arc::new(ShapeEntityConverter));
    registry.register("sksl".to_string(), Arc::new(SkSLEntityConverter));
}
