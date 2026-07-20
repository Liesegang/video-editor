//! Plugin system for effects, loaders, exporters, property evaluators, and more.

#![allow(
    improper_ctypes_definitions,
    reason = "dynamic plugins intentionally exchange Rust trait objects and require an exact same-toolchain ABI"
)]

pub mod entity_converter;
mod evaluator;
mod manager;
mod operation;
mod repository;
mod runtime_native;
mod traits;

pub mod decorators;
pub mod effectors;
pub mod effects;
pub mod exporters;
pub mod loaders;
pub mod properties;
pub mod styles;

// Re-export from submodules
pub use entity_converter::{EntityConverterPlugin, FrameEvaluationContext, ResolvedNodeInputs};
pub use evaluator::{
    EvaluationContext, PropertyEvaluationDiagnostic, PropertyEvaluationError,
    PropertyEvaluationOutcome, PropertyEvaluator, PropertyEvaluatorRegistry,
};
pub use manager::{PluginInfo, PluginManager};
pub(crate) use operation::OperationNodeParts;
pub use operation::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY,
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, OperationDescriptor, OperationDescriptorError,
    PROPERTY_PORT_PREFIX, STYLE_APPLY_OPERATION, STYLE_CATEGORY, property_name_from_port,
    property_port_key, property_ui_type_to_port_data_type,
};
pub use repository::PluginRepository;
pub use runtime_native::{RuntimePluginDescriptor, RuntimePluginScanReport};
pub use ruvie_plugin_api as native_plugin_api;
pub use traits::{Plugin, PropertyPlugin};

// Re-export from sub-crate modules
pub use decorators::DecoratorPlugin;
pub use effectors::EffectorPlugin;
pub use effects::{EffectDefinition, EffectPlugin, EffectRepository};
pub use exporters::{ExportFormat, ExportPlugin, ExportRepository, ExportSettings};
pub use loaders::{
    AssetMetadata, LoadPlugin, LoadPluginError, LoadPluginResult, LoadRepository, LoadRequest,
    LoadResponse,
};
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
// pub use loaders::ffmpeg_video::FfmpegVideoLoader;
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
