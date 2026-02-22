//! Node type registration.
//!
//! Canonical definitions live in `crate::nodes`. This module provides
//! the registration entry point used by `PluginManager`.

use crate::plugin::PluginManager;

/// Register all built-in node type definitions.
pub(crate) fn register_all_node_types(manager: &PluginManager) {
    for def in crate::nodes::all_definitions() {
        manager.register_node_type(def);
    }
}
