//! Plugin system for effects, loaders, exporters, property evaluators, and more.

#![allow(improper_ctypes_definitions)]

mod evaluator;
mod manager;
mod repository;
mod traits;

pub mod effects;
pub mod exporters;
pub mod loaders;
pub mod properties;

// Re-export from submodules
pub use evaluator::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
pub use manager::{PluginInfo, PluginManager};
pub use repository::PluginRepository;
pub use traits::{InspectorPlugin, InspectorPluginCreateFn, Plugin, PropertyPlugin};

// Re-export from sub-crate modules
pub use effects::{EffectDefinition, EffectPlugin, EffectRepository};
pub use exporters::{ExportFormat, ExportPlugin, ExportRepository, ExportSettings};
pub use loaders::{AssetMetadata, LoadPlugin, LoadRepository, LoadRequest, LoadResponse};

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
    Inspector,
}

// FFI function types for dynamic loading
use crate::framing::entity_converters::EntityConverterPlugin;
pub type PropertyPluginCreateFn = unsafe extern "C" fn() -> *mut dyn PropertyPlugin;
pub type EffectPluginCreateFn = unsafe extern "C" fn() -> *mut dyn EffectPlugin;
pub type LoadPluginCreateFn = unsafe extern "C" fn() -> *mut dyn LoadPlugin;
pub type ExportPluginCreateFn = unsafe extern "C" fn() -> *mut dyn ExportPlugin;
pub type EntityConverterPluginCreateFn = unsafe extern "C" fn() -> *mut dyn EntityConverterPlugin;
