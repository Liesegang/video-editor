use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StyleInstance {
    pub id: Uuid,
    pub style_type: String, // "fill" or "stroke"
    #[serde(default)] // Ensure backward compatibility if deserializing from older JSON (though this is new struct)
    pub properties: PropertyMap,
}

impl std::hash::Hash for StyleInstance {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for StyleInstance {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for StyleInstance {}

impl StyleInstance {
    pub fn new(style_type: &str, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            style_type: style_type.to_string(),
            properties,
        }
    }
}
