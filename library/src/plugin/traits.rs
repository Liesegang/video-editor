//! Core plugin traits.

use std::sync::Arc;

use crate::plugin::{PluginCategory, PropertyEvaluator};

/// Base trait for all plugins.
pub trait Plugin: Send + Sync {
    /// Stable identifier supplied by the plugin descriptor.
    ///
    /// Unlike the former `&'static str` contract, this permits identifiers
    /// discovered from a runtime-loaded native plugin without leaking memory.
    fn id(&self) -> &str;
    fn name(&self) -> String;
    fn category(&self) -> String;
    fn version(&self) -> (u32, u32, u32);
    fn impl_type(&self) -> String {
        "Native".to_string()
    }
}

/// Plugin trait for property evaluators.
pub trait PropertyPlugin: Plugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Property
    }
}
