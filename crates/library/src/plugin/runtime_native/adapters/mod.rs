pub(super) mod decorator;
pub(super) mod effect;
mod effector;
pub(super) mod loader;
pub(super) mod property;
pub(super) mod style;

pub(super) use decorator::{RuntimeDecoratorPlugin, RuntimeDecoratorProtocol};
pub(super) use effect::RuntimeEffectPlugin;
pub(super) use effector::RuntimeEffectorPlugin;
pub(super) use loader::RuntimeLoaderPlugin;
pub(super) use property::RuntimePropertyEvaluator;
pub(super) use style::RuntimeStylePlugin;

use std::collections::BTreeMap;

use ruvie_plugin_api::PropertyValueV1;

use super::property_wire::property_value_to_wire;
use crate::model::property::PropertyDefinition;
pub(super) fn evaluated_config_properties(
    definitions: &[PropertyDefinition],
    evaluated: &std::collections::HashMap<String, crate::model::property::PropertyValue>,
    operation_label: &str,
) -> Option<BTreeMap<String, PropertyValueV1>> {
    let mut wire_properties = BTreeMap::new();
    for definition in definitions {
        let Some(value) = evaluated.get(definition.name()) else {
            log::error!(
                "{operation_label} did not resolve declared property '{}'",
                definition.name()
            );
            return None;
        };
        let value = match property_value_to_wire(value) {
            Ok(value) => value,
            Err(error) => {
                log::error!(
                    "{operation_label} property '{}' cannot cross ABI v1: {error}",
                    definition.name()
                );
                return None;
            }
        };
        wire_properties.insert(definition.name().to_string(), value);
    }
    Some(wire_properties)
}

pub(super) fn parse_semver_triplet(value: &str) -> (u32, u32, u32) {
    let mut parts = value.split('.');
    let parse = |part: Option<&str>| part.and_then(|value| value.parse().ok()).unwrap_or(0);
    (
        parse(parts.next()),
        parse(parts.next()),
        parse(parts.next()),
    )
}
