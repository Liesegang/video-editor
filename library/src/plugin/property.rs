use super::{Plugin, PluginCategory, PropertyPlugin};
use crate::framing::property::{PropertyEvaluatorRegistry, register_builtin_evaluators};

pub struct BuiltinPropertyPlugin;

impl Plugin for BuiltinPropertyPlugin {
    fn id(&self) -> &'static str {
        "builtin_property_evaluators"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Property
    }
}

impl PropertyPlugin for BuiltinPropertyPlugin {
    fn register(&self, registry: &mut PropertyEvaluatorRegistry) {
        register_builtin_evaluators(registry);
    }
}
