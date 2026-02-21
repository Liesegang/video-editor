use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
pub struct StyleInstance {
    pub id: Uuid,
    pub style_type: String, // "fill" or "stroke"
    #[serde(default)]
    // Ensure backward compatibility if deserializing from older JSON (though this is new struct)
    pub properties: PropertyMap,
}

impl StyleInstance {
    pub fn new(style_type: &str, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            style_type: style_type.to_string(),
            properties,
        }
    }

    pub fn new_with_id(id: Uuid, style_type: &str, properties: PropertyMap) -> Self {
        Self {
            id,
            style_type: style_type.to_string(),
            properties,
        }
    }
}
