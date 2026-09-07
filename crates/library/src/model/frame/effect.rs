use crate::model::frame::entity::StyleConfig;
use crate::model::property::PropertyValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImageEffect {
    Plugin {
        effect_type: String,
        #[serde(default)]
        properties: HashMap<String, PropertyValue>,
    },
    /// One typed Image -> Image layer-style stage. Graph nesting, rather than
    /// a separately sorted appearance stack, owns the application order.
    LayerStyle(StyleConfig),
}

impl Hash for ImageEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Plugin {
                effect_type,
                properties,
            } => {
                effect_type.hash(state);
                let mut entries: Vec<_> = properties.iter().collect();
                entries.sort_by_key(|(key, _)| key.as_str());
                for (key, value) in entries {
                    key.hash(state);
                    value.hash(state);
                }
            }
            Self::LayerStyle(style) => style.hash(state),
        }
    }
}
