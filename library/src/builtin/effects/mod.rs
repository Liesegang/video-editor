macro_rules! define_effect_plugin {
    (
        $struct_name:ident,
        id: $id:expr,
        name: $name:expr,
        category: $category:expr,
        version: ($major:expr, $minor:expr, $patch:expr)
    ) => {
        pub struct $struct_name;

        impl $struct_name {
            pub fn new() -> Self {
                Self
            }
        }

        impl $crate::plugin::Plugin for $struct_name {
            fn id(&self) -> &'static str {
                $id
            }

            fn name(&self) -> String {
                $name.to_string()
            }

            fn category(&self) -> String {
                $category.to_string()
            }

            fn version(&self) -> (u32, u32, u32) {
                ($major, $minor, $patch)
            }
        }
    };
}
pub(crate) use define_effect_plugin;

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
use crate::plugin::{Plugin, PluginCategory};
use crate::project::property::{PropertyDefinition, PropertyValue};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct EffectDefinition {
    pub label: String,
    pub properties: Vec<PropertyDefinition>,
}

pub trait EffectPlugin: Plugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError>;

    fn properties(&self) -> Vec<PropertyDefinition>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effect
    }
}

pub struct EffectRepository {
    pub plugins: HashMap<String, Arc<dyn EffectPlugin>>,
}

impl EffectRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn EffectPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn EffectPlugin>> {
        self.plugins.get(id)
    }
}
