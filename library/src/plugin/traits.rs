//! Core plugin traits.

use std::sync::Arc;

use crate::model::project::TrackClipKind;
use crate::model::project::property::PropertyDefinition;
use crate::plugin::{PluginCategory, PropertyEvaluator};

/// Base trait for all plugins.
pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;
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

/// Plugin trait for inspector definitions.
pub trait InspectorPlugin: Plugin {
    fn get_definitions(&self, kind: &TrackClipKind) -> Vec<PropertyDefinition>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Inspector
    }
}

// Create function types for dynamic loading
pub type InspectorPluginCreateFn = unsafe extern "C" fn() -> *mut dyn InspectorPlugin;
