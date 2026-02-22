//! Plugin system infrastructure: traits, registry, and manager.

#![allow(improper_ctypes_definitions)]

mod evaluator;
mod manager;
pub mod node_definitions;
pub mod node_types;
mod repository;
mod traits;

// Re-export from submodules
pub use evaluator::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
pub use manager::{PluginInfo, PluginManager};
pub use node_types::{NodeCategory, NodeTypeDefinition};
pub use repository::PluginRepository;
pub use traits::{Plugin, PropertyPlugin};

// Re-export from builtin for backward compatibility
pub use crate::builtin::decorators::DecoratorPlugin;
pub use crate::builtin::effectors::EffectorPlugin;
pub use crate::builtin::effects::{EffectDefinition, EffectPlugin, EffectRepository};
pub use crate::builtin::exporters::{ExportFormat, ExportPlugin, ExportRepository, ExportSettings};
pub use crate::builtin::loaders::{
    AssetMetadata, LoadPlugin, LoadRepository, LoadRequest, LoadResponse,
};
pub use crate::builtin::styles::StylePlugin;

// Re-export concrete implementations for backward compatibility
pub use crate::builtin::effects::blur::BlurEffectPlugin;
pub use crate::builtin::effects::dilate::DilateEffectPlugin;
pub use crate::builtin::effects::drop_shadow::DropShadowEffectPlugin;
pub use crate::builtin::effects::erode::ErodeEffectPlugin;
pub use crate::builtin::effects::magnifier::MagnifierEffectPlugin;
pub use crate::builtin::effects::tile::TileEffectPlugin;
pub use crate::builtin::exporters::ffmpeg_export::FfmpegExportPlugin;
pub use crate::builtin::exporters::png_export::PngExportPlugin;
pub use crate::builtin::loaders::ffmpeg_video::FfmpegVideoLoader;
pub use crate::builtin::loaders::native_image::NativeImageLoader;
pub use crate::builtin::properties::{
    ConstantPropertyPlugin, ExpressionPropertyPlugin, KeyframePropertyPlugin,
};

// Plugin category enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
    Effect,
    Load,
    Export,
    Property,
    Effector,
    Decorator,
    Style,
}
