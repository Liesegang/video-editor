use std::collections::BTreeMap;

use ruvie_plugin_api::{
    EFFECTOR_EVALUATE_V1, EffectorEvaluateRequestV1, EffectorOutputV1, EffectorTargetV1,
    OpacityModeV1,
};

use super::super::abi::RuntimeComponent;
use super::parse_semver_triplet;
use crate::model::property::PropertyDefinition;
use crate::plugin::{EffectorPlugin, EvaluatedOperation, Plugin, PluginCategory};
pub(in crate::plugin::runtime_native) struct RuntimeEffectorPlugin {
    pub(in crate::plugin::runtime_native) component: RuntimeComponent,
    pub(in crate::plugin::runtime_native) definitions: Vec<PropertyDefinition>,
}

impl Plugin for RuntimeEffectorPlugin {
    fn id(&self) -> &str {
        &self.component.descriptor.id
    }

    fn name(&self) -> String {
        self.component.descriptor.name.clone()
    }

    fn category(&self) -> String {
        self.component.descriptor.group.clone()
    }

    fn version(&self) -> (u32, u32, u32) {
        parse_semver_triplet(&self.component.descriptor.version)
    }

    fn impl_type(&self) -> String {
        "Native ABI v1".to_string()
    }
}

impl EffectorPlugin for RuntimeEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        self.definitions.clone()
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: uuid::Uuid,
    ) -> Option<crate::core::ensemble::types::EffectorConfig> {
        let mut properties = BTreeMap::new();
        for (name, value) in context.properties() {
            properties.insert(name.clone(), serde_json::Value::from(value));
        }
        let payload = match serde_json::to_value(EffectorEvaluateRequestV1 {
            time: context.time(),
            properties,
        }) {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("Failed to encode runtime effector '{}': {error}", self.id());
                return None;
            }
        };
        let response = match self.component.invoke(EFFECTOR_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => {
                log::error!("Runtime effector '{}' failed: {error}", self.id());
                return None;
            }
        };
        let output: EffectorOutputV1 = match serde_json::from_value(response) {
            Ok(output) => output,
            Err(error) => {
                log::error!(
                    "Runtime effector '{}' returned an invalid response: {error}",
                    self.id()
                );
                return None;
            }
        };
        match output {
            EffectorOutputV1::NoOutput => None,
            EffectorOutputV1::Transform {
                translate,
                rotate,
                scale,
                target,
            } => {
                if !translate.0.is_finite()
                    || !translate.1.is_finite()
                    || !rotate.is_finite()
                    || !scale.0.is_finite()
                    || !scale.1.is_finite()
                {
                    log::error!(
                        "Runtime effector '{}' returned non-finite values",
                        self.id()
                    );
                    return None;
                }
                Some(crate::core::ensemble::types::EffectorConfig::Transform {
                    translate,
                    rotate,
                    scale,
                    target: convert_target(target),
                })
            }
            EffectorOutputV1::Opacity {
                opacity,
                mode,
                target,
            } => {
                if !opacity.is_finite() {
                    log::error!(
                        "Runtime effector '{}' returned non-finite opacity",
                        self.id()
                    );
                    return None;
                }
                Some(crate::core::ensemble::types::EffectorConfig::Opacity {
                    target_opacity: opacity,
                    mode: match mode {
                        OpacityModeV1::Set => crate::core::ensemble::effectors::OpacityMode::Set,
                        OpacityModeV1::Add => crate::core::ensemble::effectors::OpacityMode::Add,
                        OpacityModeV1::Multiply => {
                            crate::core::ensemble::effectors::OpacityMode::Multiply
                        }
                    },
                    target: convert_target(target),
                })
            }
        }
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effector
    }
}

fn convert_target(value: EffectorTargetV1) -> crate::core::ensemble::target::EffectorTarget {
    match value {
        EffectorTargetV1::Block => crate::core::ensemble::target::EffectorTarget::Block,
        EffectorTargetV1::Line => crate::core::ensemble::target::EffectorTarget::Line,
        EffectorTargetV1::Char => crate::core::ensemble::target::EffectorTarget::Char,
    }
}
