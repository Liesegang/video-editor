use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StyleInstance {
    pub id: Uuid,
    pub style_type: String, // "fill" or "stroke"
    #[serde(default)] // Ensure backward compatibility if deserializing from older JSON (though this is new struct)
    pub properties: PropertyMap,
}

impl Eq for StyleInstance {}

impl std::hash::Hash for StyleInstance {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.style_type.hash(state);
        // PropertyMap doesn't implement Hash, so we might skip it or use a workaround.
        // For ReorderableList, we mostly care about identity (ID).
        // If properties change, it's still the same style instance in the list logic.
        // So hashing ID should be sufficient for list diffing.
    }
}

impl StyleInstance {
    pub fn new(style_type: &str, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            style_type: style_type.to_string(),
            properties,
        }
    }
}
