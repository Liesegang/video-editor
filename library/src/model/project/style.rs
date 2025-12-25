use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StyleInstance {
    pub id: Uuid,
    pub style_type: String, // "fill" or "stroke"
    #[serde(default)]
    // Ensure backward compatibility if deserializing from older JSON (though this is new struct)
    pub properties: PropertyMap,
}

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

    /// Update or upsert a style property value/keyframe.
    pub fn update_property_or_keyframe(
        &mut self,
        key: &str,
        time: f64,
        value: crate::model::project::property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) {
        use crate::model::project::property::Property;
        if let Some(prop) = self.properties.get_mut(key) {
            if prop.evaluator == "keyframe" {
                prop.upsert_keyframe(time, value, easing);
            } else {
                self.properties
                    .set(key.to_string(), Property::constant(value));
            }
        } else {
            self.properties
                .set(key.to_string(), Property::constant(value));
        }
    }
}
