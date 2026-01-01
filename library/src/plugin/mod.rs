//! Plugin system for effects, loaders, exporters, property evaluators, and more.

#![allow(improper_ctypes_definitions)]

pub mod entity_converter;
mod evaluator;
mod manager;
mod repository;
mod traits;

pub mod decorators;
pub mod effectors;
pub mod effects;
pub mod exporters;
pub mod loaders;
pub mod properties;
pub mod styles;

// Re-export from submodules
pub use entity_converter::{EntityConverterPlugin, FrameEvaluationContext};
pub use evaluator::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
pub use manager::{PluginInfo, PluginManager};
pub use repository::PluginRepository;
pub use traits::{Plugin, PropertyPlugin};

// Re-export from sub-crate modules
pub use decorators::DecoratorPlugin;
pub use effectors::EffectorPlugin;
pub use effects::{EffectDefinition, EffectPlugin, EffectRepository};
pub use exporters::{ExportFormat, ExportPlugin, ExportRepository, ExportSettings};
pub use loaders::{AssetMetadata, LoadPlugin, LoadRepository, LoadRequest, LoadResponse};
pub use styles::StylePlugin;

// Re-export plugin implementations
pub use effects::blur::BlurEffectPlugin;
pub use effects::dilate::DilateEffectPlugin;
pub use effects::drop_shadow::DropShadowEffectPlugin;
pub use effects::erode::ErodeEffectPlugin;
pub use effects::magnifier::MagnifierEffectPlugin;
pub use effects::tile::TileEffectPlugin;
pub use exporters::ffmpeg_export::FfmpegExportPlugin;
pub use exporters::png_export::PngExportPlugin;
pub use loaders::ffmpeg_video::FfmpegVideoLoader;
pub use loaders::native_image::NativeImageLoader;
pub use properties::{ConstantPropertyPlugin, ExpressionPropertyPlugin, KeyframePropertyPlugin};

// Plugin category enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
    Effect,
    Load,
    Export,
    Property,
    EntityConverter,
    Effector,
    Decorator,
    Style,
}
