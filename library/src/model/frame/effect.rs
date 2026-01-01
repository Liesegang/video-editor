use crate::model::project::property::PropertyValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ImageEffect {
    #[serde(rename = "type")]
    pub effect_type: String,
    #[serde(default)]
    pub properties: HashMap<String, PropertyValue>,
}

impl Hash for ImageEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.effect_type.hash(state);
        let mut entries: Vec<_> = self.properties.iter().collect();
        entries.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in entries {
            k.hash(state);
            v.hash(state);
        }
    }
}
