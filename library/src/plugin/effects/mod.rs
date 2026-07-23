pub mod blur;
pub mod dilate;
pub mod drop_shadow;
pub mod erode;
pub mod magnifier;
pub mod pixel_sorter;
pub mod sksl_plugin;
pub mod tile;
pub mod utils;

pub use self::blur::BlurEffectPlugin;
pub use self::dilate::DilateEffectPlugin;
pub use self::drop_shadow::DropShadowEffectPlugin;
pub use self::erode::ErodeEffectPlugin;
pub use self::magnifier::MagnifierEffectPlugin;
pub use self::pixel_sorter::PixelSorterPlugin;
pub use self::sksl_plugin::SkslEffectPlugin;
pub use self::tile::TileEffectPlugin;

use crate::error::LibraryError;
use crate::model::property::{PropertyDefinition, PropertyValue};
use crate::plugin::{OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct EffectDefinition {
    pub label: String,
    pub properties: Vec<PropertyDefinition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectColorDomain {
    /// The effect consumes the historical encoded straight-sRGBA8 ABI.
    UnmanagedSrgba8Only,
    /// The effect preserves premultiplied RGBAF32 samples and their exact
    /// `WorkingColorIdentity` without applying a display/transfer transform.
    ProjectLinearPreserving,
}

pub trait EffectPlugin: Plugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError>;

    fn properties(&self) -> Vec<PropertyDefinition>;

    fn color_domain(&self) -> EffectColorDomain {
        EffectColorDomain::UnmanagedSrgba8Only
    }

    /// Authored color parameters which must be converted to an exact
    /// Project-working [`PropertyValue::ColorValue`] before this effect runs
    /// on a project-linear frame.
    ///
    /// This is deliberately opt-in so native effects retaining their legacy
    /// `PropertyValue::Color` ABI are never mutated as a side effect of some
    /// other plugin's color contract.
    fn project_linear_color_parameters(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Authoritative graph operation description. Existing native effects
    /// keep their property-definition implementation as the compatibility
    /// source while all Node construction and execution consumes this common
    /// descriptor.
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::effect(self.id(), self.name(), self.properties())
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effect
    }
}

#[derive(Default)]
pub struct EffectRepository {
    pub plugins: HashMap<String, Arc<dyn EffectPlugin>>,
}

impl EffectRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Arc<dyn EffectPlugin>) -> Option<Arc<dyn EffectPlugin>> {
        self.plugins.insert(plugin.id().to_string(), plugin)
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn EffectPlugin>> {
        self.plugins.get(id)
    }
}
