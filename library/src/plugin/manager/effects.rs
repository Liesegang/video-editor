//! Effect lookup, working-color enforcement, and invocation.

use std::collections::HashMap;

use crate::error::LibraryError;
use crate::model::property::PropertyValue;
use crate::plugin::effects::{EffectColorDomain, EffectDefinition};
use crate::plugin::{EFFECT_APPLY_OPERATION, EFFECT_CATEGORY};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;

use super::PluginManager;

impl PluginManager {
    pub fn apply_effect(
        &self,
        key: &str,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let plugin = {
            let inner = self.read_registry();
            inner.effect_plugins.get(key).cloned()
        };
        if let Some(plugin) = plugin {
            let working_identity = match input {
                RenderOutput::Working(image) => Some(image.identity().clone()),
                RenderOutput::Image(_) | RenderOutput::Texture(_) => None,
            };
            if working_identity.is_some()
                && plugin.color_domain() != EffectColorDomain::ProjectLinearPreserving
            {
                return Err(LibraryError::Plugin(format!(
                    "Effect '{key}' supports only the unmanaged encoded-sRGBA8 boundary; it cannot process a Project linear RGBAF32 frame"
                )));
            }
            log::debug!("PluginManager: Applying effect '{}'", key);
            let output = plugin.apply(input, params, gpu_context)?;
            if let Some(expected) = working_identity {
                match &output {
                    RenderOutput::Working(image) if image.identity() == &expected => {}
                    RenderOutput::Working(image) => {
                        return Err(LibraryError::Plugin(format!(
                            "Effect '{key}' changed Project working identity from {expected:?} to {:?}",
                            image.identity()
                        )));
                    }
                    RenderOutput::Image(_) | RenderOutput::Texture(_) => {
                        return Err(LibraryError::Plugin(format!(
                            "Effect '{key}' dropped the Project working RGBAF32 contract"
                        )));
                    }
                }
            }
            Ok(output)
        } else if matches!(input, RenderOutput::Working(_)) {
            Err(LibraryError::Plugin(format!(
                "Effect '{key}' is unavailable; refusing to bypass it in a Project linear render"
            )))
        } else {
            log::warn!("Effect '{}' not found", key);
            Ok(input.clone())
        }
    }

    pub(crate) fn effect_project_linear_color_parameters(&self, key: &str) -> Vec<String> {
        let inner = self.read_registry();
        inner
            .effect_plugins
            .get(key)
            .map(|plugin| {
                plugin
                    .project_linear_color_parameters()
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_effect_definition(&self, effect_id: &str) -> Option<EffectDefinition> {
        let descriptor =
            match self.operation_descriptor(EFFECT_CATEGORY, effect_id, EFFECT_APPLY_OPERATION) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    log::warn!("Invalid or unavailable Effect descriptor {effect_id}: {error}");
                    return None;
                }
            };
        Some(EffectDefinition {
            label: descriptor.label().to_string(),
            properties: descriptor.properties().to_vec(),
        })
    }
}
