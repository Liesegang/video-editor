//! Built-in plugin implementations.

pub mod decorators;
pub mod effectors;
pub mod effects;
pub mod entity_converter;
pub mod exporters;
pub mod loaders;
pub mod properties;
pub(crate) mod python_evaluator;
pub mod styles;

// Re-export plugin trait impls
pub use decorators::DecoratorPlugin;
pub use effectors::EffectorPlugin;
pub use effects::{EffectDefinition, EffectPlugin, EffectRepository};
pub use exporters::{ExportFormat, ExportPlugin, ExportRepository, ExportSettings};
pub use loaders::{AssetMetadata, LoadPlugin, LoadRepository, LoadRequest, LoadResponse};
pub use styles::StylePlugin;

// Re-export concrete implementations
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
